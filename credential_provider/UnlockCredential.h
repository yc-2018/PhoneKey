#pragma once

#include <windows.h>
#include <credentialprovider.h>

// Field indices for the credential tile
enum UNLOCK_FIELD_ID {
    UFI_TILEIMAGE   = 0,
    UFI_LABEL       = 1,
    UFI_USERNAME    = 2,
    UFI_PASSWORD    = 3,
    UFI_SUBMIT      = 4,
    UFI_NUM_FIELDS  = 5,
};

// V1 credential only - no ICredentialProviderCredential2
// This avoids LogonUI expecting ICredentialProviderSetUserArray on the provider.
class CUnlockCredential : public ICredentialProviderCredential {
public:
    CUnlockCredential();

    // Initialize with the usage scenario
    HRESULT Initialize(
        _In_ CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus);

    // Called by the pipe listener to set credentials for auto-logon
    void SetCredentials(
        _In_ PCWSTR pszDomain,
        _In_ PCWSTR pszUsername,
        _In_ PCWSTR pszPassword);

    // Called by the provider to clear auto-logon flag
    void ClearPendingCredentials();

    // Called by the pipe listener with the app's entry-time proximity result
    void SetNearby(_In_ bool bNearby);

    // Check if credentials are pending (set by pipe but not yet consumed)
    bool HasPendingCredentials() const;

    // IUnknown
    IFACEMETHODIMP QueryInterface(_In_ REFIID riid, _Outptr_ void** ppv);
    IFACEMETHODIMP_(ULONG) AddRef();
    IFACEMETHODIMP_(ULONG) Release();

    // ICredentialProviderCredential
    IFACEMETHODIMP Advise(_In_ ICredentialProviderCredentialEvents* pcpce);
    IFACEMETHODIMP UnAdvise();
    IFACEMETHODIMP SetSelected(_Out_ BOOL* pbAutoLogon);
    IFACEMETHODIMP SetDeselected();
    IFACEMETHODIMP GetFieldState(
        _In_ DWORD dwFieldID,
        _Out_ CREDENTIAL_PROVIDER_FIELD_STATE* pcpfs,
        _Out_ CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE* pcpfis);
    IFACEMETHODIMP GetStringValue(
        _In_ DWORD dwFieldID, _Outptr_result_nullonfailure_ PWSTR* ppwsz);
    IFACEMETHODIMP GetBitmapValue(
        _In_ DWORD dwFieldID, _Outptr_result_nullonfailure_ HBITMAP* phbmp);
    IFACEMETHODIMP GetCheckboxValue(
        _In_ DWORD dwFieldID, _Out_ BOOL* pbChecked, _Outptr_result_nullonfailure_ PWSTR* ppwszLabel);
    IFACEMETHODIMP GetSubmitButtonValue(
        _In_ DWORD dwFieldID, _Out_ DWORD* pdwAdjacentTo);
    IFACEMETHODIMP GetComboBoxValueCount(
        _In_ DWORD dwFieldID, _Out_ DWORD* pcItems, _Out_ DWORD* pdwSelectedItem);
    IFACEMETHODIMP GetComboBoxValueAt(
        _In_ DWORD dwFieldID, _In_ DWORD dwItem, _Outptr_result_nullonfailure_ PWSTR* ppwszItem);
    IFACEMETHODIMP SetStringValue(
        _In_ DWORD dwFieldID, _In_ PCWSTR pwz);
    IFACEMETHODIMP SetCheckboxValue(
        _In_ DWORD dwFieldID, _In_ BOOL bChecked);
    IFACEMETHODIMP SetComboBoxSelectedValue(
        _In_ DWORD dwFieldID, _In_ DWORD dwSelectedItem);
    IFACEMETHODIMP CommandLinkClicked(
        _In_ DWORD dwFieldID);
    IFACEMETHODIMP GetSerialization(
        _Out_ CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE* pcpgsr,
        _Out_ CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* pcpcs,
        _Outptr_result_maybenull_ PWSTR* ppwszOptionalStatusText,
        _Out_ CREDENTIAL_PROVIDER_STATUS_ICON* pcpsiOptionalStatusIcon);
    IFACEMETHODIMP ReportResult(
        _In_ NTSTATUS ntsStatus,
        _In_ NTSTATUS ntsSubstatus,
        _Outptr_result_maybenull_ PWSTR* ppwszOptionalStatusText,
        _Out_ CREDENTIAL_PROVIDER_STATUS_ICON* pcpsiOptionalStatusIcon);

private:
    ~CUnlockCredential();

    LONG    _cRef;
    CREDENTIAL_PROVIDER_USAGE_SCENARIO _cpus;
    ICredentialProviderCredentialEvents* _pCredEvents;

    // Cached auth package ID (resolved once during Initialize)
    ULONG   _ulAuthPackage;
    bool    _bAuthPackageValid;

    // Credential fields
    WCHAR   _szUsername[256];
    WCHAR   _szPassword[256];
    WCHAR   _szDomain[256];
    bool    _bPendingCredentials;
    bool    _bNearby;
    bool    _bNearbyKnown;
    ULONGLONG _advisedTick;

    CRITICAL_SECTION _cs;
};
