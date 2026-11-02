#include <windows.h>
#include <credentialprovider.h>
#include <NTSecAPI.h>
#include <sddl.h>
#include <shlwapi.h>
#include <new>
#include <strsafe.h>

#include "UnlockCredential.h"
#include "PresenceCheck.h"
#include "guid.h"
#include "helpers.h"

extern LONG g_cRef;

// Debug logging helper
static void DbgLog(PCWSTR fmt, ...)
{
    WCHAR buf[512];
    va_list args;
    va_start(args, fmt);
    StringCchVPrintfW(buf, ARRAYSIZE(buf), fmt, args);
    va_end(args);
    OutputDebugStringW(buf);
}

// Windows may restore the last used provider when the lock screen appears.
// Selections made that early must not submit credentials on their own.
static const ULONGLONG AUTO_LOGON_GUARD_MS = 1500;

// Give the app enough time to gather a fresh BLE advertisement (its own
// verification deadline is 2.5s) while still answering LogonUI promptly.
static const DWORD PRESENCE_VERIFY_TIMEOUT_MS = 3200;

// Static field descriptors
static const CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR s_rgFieldDescriptors[UFI_NUM_FIELDS] = {
    { UFI_TILEIMAGE, CPFT_TILE_IMAGE,   L"Image",    {0} },
    { UFI_LABEL,     CPFT_LARGE_TEXT,    L"手机感应解锁", {0} },
    { UFI_USERNAME,  CPFT_EDIT_TEXT,     L"Username", {0} },
    { UFI_PASSWORD,  CPFT_PASSWORD_TEXT, L"Password", {0} },
    { UFI_SUBMIT,    CPFT_SUBMIT_BUTTON, L"Submit",  {0} },
};

// Field states for unlock/logon. The account and password are supplied by
// the app and must never be shown as editable fields on the lock screen.
static const CREDENTIAL_PROVIDER_FIELD_STATE s_rgFieldStates[UFI_NUM_FIELDS] = {
    CPFS_DISPLAY_IN_SELECTED_TILE,  // UFI_TILEIMAGE
    CPFS_DISPLAY_IN_BOTH,           // UFI_LABEL
    CPFS_HIDDEN,                    // UFI_USERNAME
    CPFS_HIDDEN,                    // UFI_PASSWORD
    CPFS_DISPLAY_IN_SELECTED_TILE,  // UFI_SUBMIT
};

CUnlockCredential::CUnlockCredential() :
    _cRef(1),
    _cpus(CPUS_INVALID),
    _pCredEvents(nullptr),
    _ulAuthPackage(0),
    _bAuthPackageValid(false),
    _bPendingCredentials(false),
    _bNearby(false),
    _bNearbyKnown(false),
    _advisedTick(0)
{
    InterlockedIncrement(&g_cRef);
    InitializeCriticalSection(&_cs);
    _szUsername[0] = L'\0';
    _szPassword[0] = L'\0';
    _szDomain[0] = L'\0';
}

CUnlockCredential::~CUnlockCredential()
{
    SecureZeroMemory(_szPassword, sizeof(_szPassword));
    SecureZeroMemory(_szUsername, sizeof(_szUsername));
    DeleteCriticalSection(&_cs);
    InterlockedDecrement(&g_cRef);
}

HRESULT CUnlockCredential::Initialize(
    _In_ CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus)
{
    _cpus = cpus;

    // Cache the Negotiate auth package ID once
    HRESULT hr = LookupNegotiatePackageId(&_ulAuthPackage);
    if (SUCCEEDED(hr)) {
        _bAuthPackageValid = true;
        DbgLog(L"[Credential] Negotiate auth package ID: %lu\n", _ulAuthPackage);
    } else {
        DbgLog(L"[Credential] WARNING: LookupNegotiatePackageId failed: 0x%08X\n", hr);
    }

    return S_OK;
}

void CUnlockCredential::SetCredentials(
    _In_ PCWSTR pszDomain,
    _In_ PCWSTR pszUsername,
    _In_ PCWSTR pszPassword)
{
    EnterCriticalSection(&_cs);
    StringCchCopyW(_szDomain, ARRAYSIZE(_szDomain), pszDomain);
    StringCchCopyW(_szUsername, ARRAYSIZE(_szUsername), pszUsername);
    StringCchCopyW(_szPassword, ARRAYSIZE(_szPassword), pszPassword);
    _bPendingCredentials = true;
    DbgLog(L"[Credential] SetCredentials: domain=%s user=%s pending=true\n", _szDomain, _szUsername);
    LeaveCriticalSection(&_cs);
}

void CUnlockCredential::ClearPendingCredentials()
{
    EnterCriticalSection(&_cs);
    _bPendingCredentials = false;
    SecureZeroMemory(_szPassword, sizeof(_szPassword));
    DbgLog(L"[Credential] ClearPendingCredentials\n");
    LeaveCriticalSection(&_cs);
}

void CUnlockCredential::SetNearby(_In_ bool bNearby)
{
    EnterCriticalSection(&_cs);
    _bNearby = bNearby;
    _bNearbyKnown = true;
    DbgLog(L"[Credential] SetNearby: near=%d pending=%d\n",
        bNearby ? 1 : 0, _bPendingCredentials ? 1 : 0);
    LeaveCriticalSection(&_cs);
}

bool CUnlockCredential::HasPendingCredentials() const
{
    return _bPendingCredentials;
}

// IUnknown

HRESULT CUnlockCredential::QueryInterface(_In_ REFIID riid, _Outptr_ void** ppv)
{
    if (!ppv) return E_INVALIDARG;

    // V1 credential only - do NOT respond to IID_ICredentialProviderCredential2
    if (riid == IID_IUnknown ||
        riid == IID_ICredentialProviderCredential) {
        *ppv = static_cast<ICredentialProviderCredential*>(this);
        AddRef();
        return S_OK;
    }

    *ppv = nullptr;
    return E_NOINTERFACE;
}

ULONG CUnlockCredential::AddRef()
{
    return InterlockedIncrement(&_cRef);
}

ULONG CUnlockCredential::Release()
{
    LONG cRef = InterlockedDecrement(&_cRef);
    if (cRef == 0) {
        delete this;
    }
    return cRef;
}

// ICredentialProviderCredential

HRESULT CUnlockCredential::Advise(_In_ ICredentialProviderCredentialEvents* pcpce)
{
    DbgLog(L"[Credential] Advise\n");
    if (_pCredEvents) {
        _pCredEvents->Release();
    }
    _pCredEvents = pcpce;
    if (_pCredEvents) {
        _pCredEvents->AddRef();
    }
    _advisedTick = GetTickCount64();
    return S_OK;
}

HRESULT CUnlockCredential::UnAdvise()
{
    DbgLog(L"[Credential] UnAdvise\n");
    if (_pCredEvents) {
        _pCredEvents->Release();
        _pCredEvents = nullptr;
    }
    return S_OK;
}

HRESULT CUnlockCredential::SetSelected(_Out_ BOOL* pbAutoLogon)
{
    EnterCriticalSection(&_cs);
    bool pending = _bPendingCredentials;
    LeaveCriticalSection(&_cs);

    // A click on the tile submits immediately, but a selection made as soon
    // as the lock screen appears must not: Windows can restore the last used
    // provider without any user action. The arrow remains available.
    bool userSelected =
        _advisedTick != 0 && GetTickCount64() - _advisedTick >= AUTO_LOGON_GUARD_MS;

    *pbAutoLogon = (pending && userSelected) ? TRUE : FALSE;
    DbgLog(
        L"[Credential] SetSelected: pending=%d userSelected=%d autoLogon=%d\n",
        pending ? 1 : 0, userSelected ? 1 : 0, *pbAutoLogon ? 1 : 0);
    return S_OK;
}

HRESULT CUnlockCredential::SetDeselected()
{
    // Pending credentials are revoked by the app (CLEAR) or by ReportResult,
    // not by tile selection changes.
    return S_OK;
}

HRESULT CUnlockCredential::GetFieldState(
    _In_ DWORD dwFieldID,
    _Out_ CREDENTIAL_PROVIDER_FIELD_STATE* pcpfs,
    _Out_ CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE* pcpfis)
{
    if (dwFieldID >= UFI_NUM_FIELDS) return E_INVALIDARG;

    *pcpfs = s_rgFieldStates[dwFieldID];
    *pcpfis = CPFIS_NONE;

    return S_OK;
}

HRESULT CUnlockCredential::GetStringValue(
    _In_ DWORD dwFieldID,
    _Outptr_result_nullonfailure_ PWSTR* ppwsz)
{
    *ppwsz = nullptr;

    HRESULT hr = E_INVALIDARG;
    switch (dwFieldID) {
    case UFI_LABEL: {
        bool pending;
        bool nearby;
        bool known;
        EnterCriticalSection(&_cs);
        pending = _bPendingCredentials;
        nearby = _bNearby;
        known = _bNearbyKnown;
        LeaveCriticalSection(&_cs);
        PCWSTR pszLabel;
        if (!pending) {
            pszLabel = L"手机感应解锁（等待应用就绪）";
        } else if (!known) {
            pszLabel = L"点击检测手机并解锁";
        } else {
            pszLabel = nearby
                ? L"手机在附近 · 点击解锁"
                : L"手机不在附近 · 点击后重新检测";
        }
        hr = SHStrDupW(pszLabel, ppwsz);
        break;
    }
    case UFI_USERNAME: {
        EnterCriticalSection(&_cs);
        hr = SHStrDupW(_szUsername, ppwsz);
        LeaveCriticalSection(&_cs);
        break;
    }
    case UFI_PASSWORD:
        hr = SHStrDupW(L"", ppwsz);
        break;
    default:
        break;
    }
    return hr;
}

HRESULT CUnlockCredential::GetBitmapValue(
    _In_ DWORD dwFieldID,
    _Outptr_result_nullonfailure_ HBITMAP* phbmp)
{
    *phbmp = nullptr;
    if (dwFieldID != UFI_TILEIMAGE) return E_INVALIDARG;
    // No custom bitmap - system will use a default icon
    return E_NOTIMPL;
}

HRESULT CUnlockCredential::GetCheckboxValue(
    _In_ DWORD /*dwFieldID*/,
    _Out_ BOOL* /*pbChecked*/,
    _Outptr_result_nullonfailure_ PWSTR* ppwszLabel)
{
    *ppwszLabel = nullptr;
    return E_NOTIMPL;
}

HRESULT CUnlockCredential::GetSubmitButtonValue(
    _In_ DWORD dwFieldID,
    _Out_ DWORD* pdwAdjacentTo)
{
    if (dwFieldID != UFI_SUBMIT) return E_INVALIDARG;
    *pdwAdjacentTo = UFI_LABEL;
    return S_OK;
}

HRESULT CUnlockCredential::GetComboBoxValueCount(
    _In_ DWORD /*dwFieldID*/,
    _Out_ DWORD* pcItems,
    _Out_ DWORD* pdwSelectedItem)
{
    *pcItems = 0;
    *pdwSelectedItem = 0;
    return E_NOTIMPL;
}

HRESULT CUnlockCredential::GetComboBoxValueAt(
    _In_ DWORD /*dwFieldID*/,
    _In_ DWORD /*dwItem*/,
    _Outptr_result_nullonfailure_ PWSTR* ppwszItem)
{
    *ppwszItem = nullptr;
    return E_NOTIMPL;
}

HRESULT CUnlockCredential::SetStringValue(
    _In_ DWORD dwFieldID,
    _In_ PCWSTR pwz)
{
    EnterCriticalSection(&_cs);
    HRESULT hr = S_OK;
    switch (dwFieldID) {
    case UFI_USERNAME:
        StringCchCopyW(_szUsername, ARRAYSIZE(_szUsername), pwz);
        break;
    case UFI_PASSWORD:
        StringCchCopyW(_szPassword, ARRAYSIZE(_szPassword), pwz);
        break;
    default:
        hr = E_INVALIDARG;
        break;
    }
    LeaveCriticalSection(&_cs);
    return hr;
}

HRESULT CUnlockCredential::SetCheckboxValue(
    _In_ DWORD /*dwFieldID*/,
    _In_ BOOL /*bChecked*/)
{
    return E_NOTIMPL;
}

HRESULT CUnlockCredential::SetComboBoxSelectedValue(
    _In_ DWORD /*dwFieldID*/,
    _In_ DWORD /*dwSelectedItem*/)
{
    return E_NOTIMPL;
}

HRESULT CUnlockCredential::CommandLinkClicked(_In_ DWORD /*dwFieldID*/)
{
    return E_NOTIMPL;
}

HRESULT CUnlockCredential::GetSerialization(
    _Out_ CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE* pcpgsr,
    _Out_ CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* pcpcs,
    _Outptr_result_maybenull_ PWSTR* ppwszOptionalStatusText,
    _Out_ CREDENTIAL_PROVIDER_STATUS_ICON* pcpsiOptionalStatusIcon)
{
    DbgLog(L"[Credential] GetSerialization called\n");

    *pcpgsr = CPGSR_NO_CREDENTIAL_NOT_FINISHED;
    *ppwszOptionalStatusText = nullptr;
    *pcpsiOptionalStatusIcon = CPSI_NONE;
    ZeroMemory(pcpcs, sizeof(*pcpcs));

    if (!_bAuthPackageValid) {
        DbgLog(L"[Credential] ERROR: Auth package not resolved\n");
        return E_UNEXPECTED;
    }

    EnterCriticalSection(&_cs);
    bool pending = _bPendingCredentials;
    LeaveCriticalSection(&_cs);

    if (!pending) {
        // No grant from the app. Submit arrows clicked in this state must
        // not reach LSA.
        *pcpsiOptionalStatusIcon = CPSI_WARNING;
        SHStrDupW(L"手机解锁尚未就绪，请使用 PIN 或稍后重试", ppwszOptionalStatusText);
        DbgLog(L"[Credential] GetSerialization: no pending credentials\n");
        return S_OK;
    }

    // Click-time proximity check. The app only answers OK from a fresh BLE
    // observation; every pipe error or timeout fails closed.
    if (!IsPhoneNearby(PRESENCE_VERIFY_TIMEOUT_MS)) {
        EnterCriticalSection(&_cs);
        _bNearby = false;
        _bNearbyKnown = true;
        LeaveCriticalSection(&_cs);
        *pcpsiOptionalStatusIcon = CPSI_WARNING;
        SHStrDupW(L"手机不在附近，请靠近后重试或使用 PIN", ppwszOptionalStatusText);
        DbgLog(L"[Credential] GetSerialization: phone not nearby, pending kept\n");
        return S_OK;
    }

    // Re-check under the lock: the app may have withdrawn the offer while
    // the verification was in flight.
    WCHAR szDomain[256];
    WCHAR szUsername[256];
    WCHAR szPassword[256];
    EnterCriticalSection(&_cs);
    pending = _bPendingCredentials;
    if (pending) {
        _bNearby = true;
        _bNearbyKnown = true;
        StringCchCopyW(szDomain, ARRAYSIZE(szDomain), _szDomain);
        StringCchCopyW(szUsername, ARRAYSIZE(szUsername), _szUsername);
        StringCchCopyW(szPassword, ARRAYSIZE(szPassword), _szPassword);
    }
    LeaveCriticalSection(&_cs);

    if (!pending) {
        SecureZeroMemory(szPassword, sizeof(szPassword));
        *pcpsiOptionalStatusIcon = CPSI_WARNING;
        SHStrDupW(L"手机解锁选项已撤销，请使用 PIN 或重新锁屏", ppwszOptionalStatusText);
        DbgLog(L"[Credential] GetSerialization: offer withdrawn during verify\n");
        return S_OK;
    }

    // If domain is empty, use "."
    if (szDomain[0] == L'\0') {
        StringCchCopyW(szDomain, ARRAYSIZE(szDomain), L".");
    }

    DbgLog(L"[Credential] Serializing after nearby check: domain=%s user=%s cpus=%d\n",
        szDomain, szUsername, (int)_cpus);

    // Initialize the KERB structure
    KERB_INTERACTIVE_UNLOCK_LOGON kiul;
    HRESULT hr = KerbInteractiveUnlockLogonInit(
        szDomain, szUsername, szPassword, _cpus, &kiul);
    if (FAILED(hr)) {
        DbgLog(L"[Credential] KerbInit failed: 0x%08X\n", hr);
        SecureZeroMemory(szPassword, sizeof(szPassword));
        return hr;
    }

    // Pack it into a contiguous buffer for LSA
    hr = KerbInteractiveUnlockLogonPack(kiul, &pcpcs->rgbSerialization, &pcpcs->cbSerialization);

    // Clear password from stack immediately
    SecureZeroMemory(szPassword, sizeof(szPassword));

    if (SUCCEEDED(hr)) {
        pcpcs->ulAuthenticationPackage = _ulAuthPackage;
        pcpcs->clsidCredentialProvider = CLSID_UnlockProvider;
        *pcpgsr = CPGSR_RETURN_CREDENTIAL_FINISHED;
        DbgLog(L"[Credential] Serialization OK: %lu bytes, authPkg=%lu\n",
            pcpcs->cbSerialization, _ulAuthPackage);
    } else {
        DbgLog(L"[Credential] KerbPack failed: 0x%08X\n", hr);
    }

    return hr;
}

HRESULT CUnlockCredential::ReportResult(
    _In_ NTSTATUS ntsStatus,
    _In_ NTSTATUS ntsSubstatus,
    _Outptr_result_maybenull_ PWSTR* ppwszOptionalStatusText,
    _Out_ CREDENTIAL_PROVIDER_STATUS_ICON* pcpsiOptionalStatusIcon)
{
    UNREFERENCED_PARAMETER(ntsSubstatus);

    DbgLog(L"[Credential] ReportResult: status=0x%08X substatus=0x%08X\n",
        ntsStatus, ntsSubstatus);

    *ppwszOptionalStatusText = nullptr;
    *pcpsiOptionalStatusIcon = CPSI_NONE;

    if (ntsStatus == 0) {
        // Success
        *pcpsiOptionalStatusIcon = CPSI_SUCCESS;
        DbgLog(L"[Credential] Logon SUCCESS\n");
    } else {
        *pcpsiOptionalStatusIcon = CPSI_ERROR;
        PCWSTR pszMessage = L"Logon failed.";
        switch (ntsStatus) {
        case (NTSTATUS)0xC000006DL: // STATUS_LOGON_FAILURE
            pszMessage = L"Incorrect username or password.";
            break;
        case (NTSTATUS)0xC0000072L: // STATUS_ACCOUNT_DISABLED
            pszMessage = L"Account is disabled.";
            break;
        case (NTSTATUS)0xC0000234L: // STATUS_ACCOUNT_LOCKED_OUT
            pszMessage = L"Account is locked out.";
            break;
        case (NTSTATUS)0xC0000193L: // STATUS_ACCOUNT_EXPIRED
            pszMessage = L"Account has expired.";
            break;
        case (NTSTATUS)0xC0000224L: // STATUS_PASSWORD_MUST_CHANGE
            pszMessage = L"Password must be changed.";
            break;
        }
        DbgLog(L"[Credential] Logon FAILED: %s\n", pszMessage);
        SHStrDupW(pszMessage, ppwszOptionalStatusText);
    }

    // Clear pending state after result
    ClearPendingCredentials();

    return S_OK;
}
