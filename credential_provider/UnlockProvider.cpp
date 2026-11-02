#include <windows.h>
#include <credentialprovider.h>
#include <new>
#include <strsafe.h>

#include "UnlockProvider.h"
#include "guid.h"

extern LONG g_cRef;
extern HINSTANCE g_hInstance;

// Debug logging helper - output visible in Sysinternals DebugView
static void DbgLog(PCWSTR fmt, ...)
{
    WCHAR buf[512];
    va_list args;
    va_start(args, fmt);
    StringCchVPrintfW(buf, ARRAYSIZE(buf), fmt, args);
    va_end(args);
    OutputDebugStringW(buf);
}

// Static field descriptors
static const CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR s_rgFieldDescriptors[] = {
    { 0, CPFT_TILE_IMAGE,    L"Image",           {0} },
    { 1, CPFT_LARGE_TEXT,    L"手机感应解锁",  {0} },
    { 2, CPFT_EDIT_TEXT,     L"Username",         {0} },
    { 3, CPFT_PASSWORD_TEXT, L"Password",         {0} },
    { 4, CPFT_SUBMIT_BUTTON, L"Submit",           {0} },
};

static const DWORD s_dwNumFields = ARRAYSIZE(s_rgFieldDescriptors);

// Window class name for the hidden message window
static const WCHAR s_szWndClass[] = L"UnlockProviderMsgWnd";

// -----------------------------------------------------------------------
// Hidden window proc - runs on the STA thread (LogonUI's message loop)
// -----------------------------------------------------------------------
LRESULT CALLBACK CUnlockProvider::s_WndProc(HWND hwnd, UINT uMsg, WPARAM wParam, LPARAM lParam)
{
    if (uMsg == WM_NCCREATE) {
        // Store the 'this' pointer passed via CreateWindowEx
        CREATESTRUCTW* pcs = (CREATESTRUCTW*)lParam;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (LONG_PTR)pcs->lpCreateParams);
        return TRUE;
    }

    CUnlockProvider* pThis = (CUnlockProvider*)GetWindowLongPtrW(hwnd, GWLP_USERDATA);

    if (pThis && uMsg == WM_UNLOCK_CREDENTIAL) {
        DbgLog(L"[UnlockProvider] WM_UNLOCK_CREDENTIAL received on STA thread\n");
        pThis->_FireCredentialsChanged();
        return 0;
    }

    return DefWindowProcW(hwnd, uMsg, wParam, lParam);
}

// -----------------------------------------------------------------------
// Constructor / Destructor
// -----------------------------------------------------------------------

CUnlockProvider::CUnlockProvider() :
    _cRef(1),
    _pCredential(nullptr),
    _pcpe(nullptr),
    _upAdviseContext(0),
    _cpus(CPUS_INVALID),
    _hWnd(nullptr),
    _wndClass(0),
    _bCredentialsChangedPending(false)
{
    InterlockedIncrement(&g_cRef);
    InitializeCriticalSection(&_cs);
}

CUnlockProvider::~CUnlockProvider()
{
    _pipeListener.Stop();

    if (_hWnd) {
        DestroyWindow(_hWnd);
        _hWnd = nullptr;
    }
    if (_wndClass) {
        UnregisterClassW(s_szWndClass, g_hInstance);
        _wndClass = 0;
    }
    if (_pCredential) {
        _pCredential->Release();
    }
    if (_pcpe) {
        _pcpe->Release();
    }
    DeleteCriticalSection(&_cs);
    InterlockedDecrement(&g_cRef);
}

// -----------------------------------------------------------------------
// OnUnlockCommand - called from the pipe listener thread
// -----------------------------------------------------------------------

HRESULT CUnlockProvider::OnUnlockCommand(
    _In_ PCWSTR pszDomain,
    _In_ PCWSTR pszUsername,
    _In_ PCWSTR pszPassword)
{
    DbgLog(L"[UnlockProvider] OnUnlockCommand: domain=%s user=%s\n", pszDomain, pszUsername);

    EnterCriticalSection(&_cs);

    if (!_pCredential) {
        DbgLog(L"[UnlockProvider] ERROR: No credential object\n");
        LeaveCriticalSection(&_cs);
        return E_UNEXPECTED;
    }

    // Store credentials in the credential tile
    _pCredential->SetCredentials(pszDomain, pszUsername, pszPassword);

    LeaveCriticalSection(&_cs);

    // Post message to the STA thread to fire CredentialsChanged.
    // This correctly marshals the COM call to LogonUI's apartment.
    if (_hWnd) {
        DbgLog(L"[UnlockProvider] Posting WM_UNLOCK_CREDENTIAL to STA window\n");
        PostMessageW(_hWnd, WM_UNLOCK_CREDENTIAL, 0, 0);
    } else {
        // Window not yet created or already destroyed.
        // Mark pending so Advise() can fire it.
        DbgLog(L"[UnlockProvider] No HWND, marking CredentialsChanged as pending\n");
        EnterCriticalSection(&_cs);
        _bCredentialsChangedPending = true;
        LeaveCriticalSection(&_cs);
    }

    return S_OK;
}

HRESULT CUnlockProvider::OnClearCommand()
{
    DbgLog(L"[UnlockProvider] OnClearCommand\n");

    EnterCriticalSection(&_cs);
    if (_pCredential) {
        _pCredential->ClearPendingCredentials();
    }
    LeaveCriticalSection(&_cs);

    if (_hWnd) {
        PostMessageW(_hWnd, WM_UNLOCK_CREDENTIAL, 0, 0);
    } else {
        EnterCriticalSection(&_cs);
        _bCredentialsChangedPending = true;
        LeaveCriticalSection(&_cs);
    }

    return S_OK;
}

HRESULT CUnlockProvider::OnStatusCommand(_In_ bool bNearby)
{
    DbgLog(L"[UnlockProvider] OnStatusCommand: near=%d\n", bNearby ? 1 : 0);

    EnterCriticalSection(&_cs);
    if (_pCredential) {
        _pCredential->SetNearby(bNearby);
    }
    LeaveCriticalSection(&_cs);

    if (_hWnd) {
        PostMessageW(_hWnd, WM_UNLOCK_CREDENTIAL, 0, 0);
    } else {
        EnterCriticalSection(&_cs);
        _bCredentialsChangedPending = true;
        LeaveCriticalSection(&_cs);
    }

    return S_OK;
}

// -----------------------------------------------------------------------
// _FireCredentialsChanged - always called on the STA thread
// -----------------------------------------------------------------------

void CUnlockProvider::_FireCredentialsChanged()
{
    EnterCriticalSection(&_cs);
    ICredentialProviderEvents* pcpe = _pcpe;
    UINT_PTR upContext = _upAdviseContext;
    if (pcpe) {
        pcpe->AddRef();
    }
    LeaveCriticalSection(&_cs);

    if (pcpe) {
        DbgLog(L"[UnlockProvider] Calling CredentialsChanged on STA thread\n");
        pcpe->CredentialsChanged(upContext);
        pcpe->Release();
    } else {
        DbgLog(L"[UnlockProvider] _pcpe is null, marking pending\n");
        EnterCriticalSection(&_cs);
        _bCredentialsChangedPending = true;
        LeaveCriticalSection(&_cs);
    }
}

// -----------------------------------------------------------------------
// IUnknown
// -----------------------------------------------------------------------

HRESULT CUnlockProvider::QueryInterface(_In_ REFIID riid, _Outptr_ void** ppv)
{
    if (!ppv) return E_INVALIDARG;

    if (riid == IID_IUnknown || riid == IID_ICredentialProvider) {
        *ppv = static_cast<ICredentialProvider*>(this);
        AddRef();
        return S_OK;
    }

    *ppv = nullptr;
    return E_NOINTERFACE;
}

ULONG CUnlockProvider::AddRef()
{
    return InterlockedIncrement(&_cRef);
}

ULONG CUnlockProvider::Release()
{
    LONG cRef = InterlockedDecrement(&_cRef);
    if (cRef == 0) {
        delete this;
    }
    return cRef;
}

// -----------------------------------------------------------------------
// ICredentialProvider
// -----------------------------------------------------------------------

HRESULT CUnlockProvider::SetUsageScenario(
    _In_ CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus,
    _In_ DWORD /*dwFlags*/)
{
    DbgLog(L"[UnlockProvider] SetUsageScenario: cpus=%d\n", (int)cpus);

    switch (cpus) {
    case CPUS_LOGON:
    case CPUS_UNLOCK_WORKSTATION:
    case CPUS_CREDUI:
        _cpus = cpus;
        break;
    default:
        return E_INVALIDARG;
    }

    // Create the credential tile
    _pCredential = new(std::nothrow) CUnlockCredential();
    if (!_pCredential) return E_OUTOFMEMORY;

    HRESULT hr = _pCredential->Initialize(cpus);
    if (FAILED(hr)) {
        _pCredential->Release();
        _pCredential = nullptr;
        return hr;
    }

    // Create a hidden message-only window on this thread (the STA thread).
    // This allows the pipe listener to PostMessage back to the STA.
    WNDCLASSW wc = {};
    wc.lpfnWndProc = s_WndProc;
    wc.hInstance = g_hInstance;
    wc.lpszClassName = s_szWndClass;
    _wndClass = RegisterClassW(&wc);
    if (_wndClass) {
        _hWnd = CreateWindowExW(
            0, s_szWndClass, L"", 0,
            0, 0, 0, 0,
            HWND_MESSAGE,       // Message-only window
            nullptr,
            g_hInstance,
            this);              // Pass 'this' via CREATESTRUCT
        DbgLog(L"[UnlockProvider] Created message window: hwnd=%p\n", _hWnd);
    } else {
        DbgLog(L"[UnlockProvider] WARNING: RegisterClass failed: %lu\n", GetLastError());
    }

    // Start the pipe listener
    hr = _pipeListener.Start(this);
    DbgLog(L"[UnlockProvider] PipeListener start: hr=0x%08X\n", hr);
    // If pipe fails, the credential provider still works for manual entry
    (void)hr;

    return S_OK;
}

HRESULT CUnlockProvider::SetSerialization(
    _In_ const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* /*pcpcs*/)
{
    return E_NOTIMPL;
}

HRESULT CUnlockProvider::Advise(
    _In_ ICredentialProviderEvents* pcpe,
    _In_ UINT_PTR upAdviseContext)
{
    DbgLog(L"[UnlockProvider] Advise called\n");

    EnterCriticalSection(&_cs);

    if (_pcpe) {
        _pcpe->Release();
    }
    _pcpe = pcpe;
    if (_pcpe) {
        _pcpe->AddRef();
    }
    _upAdviseContext = upAdviseContext;

    // Check if a pipe command arrived before Advise was called
    bool bPending = _bCredentialsChangedPending;
    _bCredentialsChangedPending = false;

    LeaveCriticalSection(&_cs);

    // Fire the pending CredentialsChanged - we're on the STA thread here
    if (bPending && _pcpe) {
        DbgLog(L"[UnlockProvider] Firing pending CredentialsChanged from Advise\n");
        _pcpe->CredentialsChanged(_upAdviseContext);
    }

    return S_OK;
}

HRESULT CUnlockProvider::UnAdvise()
{
    DbgLog(L"[UnlockProvider] UnAdvise called\n");

    _pipeListener.Stop();

    EnterCriticalSection(&_cs);
    if (_pcpe) {
        _pcpe->Release();
        _pcpe = nullptr;
    }
    LeaveCriticalSection(&_cs);
    return S_OK;
}

HRESULT CUnlockProvider::GetFieldDescriptorCount(_Out_ DWORD* pdwCount)
{
    *pdwCount = s_dwNumFields;
    return S_OK;
}

HRESULT CUnlockProvider::GetFieldDescriptorAt(
    _In_ DWORD dwIndex,
    _Outptr_result_nullonfailure_ CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR** ppcpfd)
{
    *ppcpfd = nullptr;

    if (dwIndex >= s_dwNumFields) return E_INVALIDARG;

    const CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR* pSrc = &s_rgFieldDescriptors[dwIndex];

    // Allocate a copy with CoTaskMemAlloc (LogonUI will CoTaskMemFree it)
    CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR* pDst =
        (CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR*)CoTaskMemAlloc(sizeof(*pDst));
    if (!pDst) return E_OUTOFMEMORY;

    pDst->dwFieldID = pSrc->dwFieldID;
    pDst->cpft = pSrc->cpft;
    pDst->guidFieldType = pSrc->guidFieldType;

    if (pSrc->pszLabel) {
        SIZE_T cbLabel = (wcslen(pSrc->pszLabel) + 1) * sizeof(WCHAR);
        pDst->pszLabel = (PWSTR)CoTaskMemAlloc(cbLabel);
        if (!pDst->pszLabel) {
            CoTaskMemFree(pDst);
            return E_OUTOFMEMORY;
        }
        CopyMemory(pDst->pszLabel, pSrc->pszLabel, cbLabel);
    } else {
        pDst->pszLabel = nullptr;
    }

    *ppcpfd = pDst;
    return S_OK;
}

HRESULT CUnlockProvider::GetCredentialCount(
    _Out_ DWORD* pdwCount,
    _Out_ DWORD* pdwDefault,
    _Out_ BOOL* pbAutoLogonWithDefault)
{
    *pdwCount = 1;

    // Never auto-logon: the tile must be selected by the user.
    // SetSelected returns TRUE once pending credentials exist, so clicking
    // the tile submits them; proximity alone must not unlock the screen.
    *pdwDefault = CREDENTIAL_PROVIDER_NO_DEFAULT;
    *pbAutoLogonWithDefault = FALSE;
    DbgLog(L"[UnlockProvider] GetCredentialCount: click-to-unlock\n");

    return S_OK;
}

HRESULT CUnlockProvider::GetCredentialAt(
    _In_ DWORD dwIndex,
    _Outptr_result_nullonfailure_ ICredentialProviderCredential** ppcpc)
{
    *ppcpc = nullptr;

    if (dwIndex != 0 || !_pCredential) return E_INVALIDARG;

    return _pCredential->QueryInterface(
        IID_ICredentialProviderCredential, (void**)ppcpc);
}
