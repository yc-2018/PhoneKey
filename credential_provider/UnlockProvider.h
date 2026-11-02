#pragma once

#include <windows.h>
#include <credentialprovider.h>
#include "UnlockCredential.h"
#include "PipeListener.h"

// Custom window message for marshaling CredentialsChanged to the STA thread
#define WM_UNLOCK_CREDENTIAL (WM_APP + 1)

class CUnlockProvider : public ICredentialProvider {
public:
    CUnlockProvider();

    // Called by the pipe listener when an UNLOCK command is received.
    // Thread-safe. Posts a message to the STA thread to trigger re-enumeration.
    HRESULT OnUnlockCommand(
        _In_ PCWSTR pszDomain,
        _In_ PCWSTR pszUsername,
        _In_ PCWSTR pszPassword);

    // Called by the pipe listener when the phone is no longer nearby.
    // Clears pending credentials so the tile can no longer unlock.
    HRESULT OnClearCommand();

    // Called by the pipe listener with the app's entry-time proximity result.
    // Only changes the tile label; every click still re-verifies.
    HRESULT OnStatusCommand(_In_ bool bNearby);

    // IUnknown
    IFACEMETHODIMP QueryInterface(_In_ REFIID riid, _Outptr_ void** ppv);
    IFACEMETHODIMP_(ULONG) AddRef();
    IFACEMETHODIMP_(ULONG) Release();

    // ICredentialProvider
    IFACEMETHODIMP SetUsageScenario(
        _In_ CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus,
        _In_ DWORD dwFlags);
    IFACEMETHODIMP SetSerialization(
        _In_ const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* pcpcs);
    IFACEMETHODIMP Advise(
        _In_ ICredentialProviderEvents* pcpe,
        _In_ UINT_PTR upAdviseContext);
    IFACEMETHODIMP UnAdvise();
    IFACEMETHODIMP GetFieldDescriptorCount(_Out_ DWORD* pdwCount);
    IFACEMETHODIMP GetFieldDescriptorAt(
        _In_ DWORD dwIndex,
        _Outptr_result_nullonfailure_ CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR** ppcpfd);
    IFACEMETHODIMP GetCredentialCount(
        _Out_ DWORD* pdwCount,
        _Out_ DWORD* pdwDefault,
        _Out_ BOOL* pbAutoLogonWithDefault);
    IFACEMETHODIMP GetCredentialAt(
        _In_ DWORD dwIndex,
        _Outptr_result_nullonfailure_ ICredentialProviderCredential** ppcpc);

private:
    ~CUnlockProvider();

    // Fire CredentialsChanged on the STA thread (called from window proc)
    void _FireCredentialsChanged();

    static LRESULT CALLBACK s_WndProc(HWND hwnd, UINT uMsg, WPARAM wParam, LPARAM lParam);

    LONG    _cRef;
    CUnlockCredential*          _pCredential;
    ICredentialProviderEvents*  _pcpe;
    UINT_PTR                    _upAdviseContext;
    CREDENTIAL_PROVIDER_USAGE_SCENARIO _cpus;
    CPipeListener               _pipeListener;

    // Hidden message-only window for STA marshaling
    HWND    _hWnd;
    ATOM    _wndClass;

    // Set when pipe command arrives before Advise
    bool    _bCredentialsChangedPending;

    CRITICAL_SECTION            _cs;
};
