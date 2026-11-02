#pragma once

#include <windows.h>
#include <NTSecAPI.h>
#include <credentialprovider.h>

// Initialize a KERB_INTERACTIVE_UNLOCK_LOGON structure with weak references
// to the input strings. Caller must keep strings alive until packing.
HRESULT KerbInteractiveUnlockLogonInit(
    _In_ PWSTR pszDomain,
    _In_ PWSTR pszUsername,
    _In_ PWSTR pszPassword,
    _In_ CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus,
    _Out_ KERB_INTERACTIVE_UNLOCK_LOGON* pkiul);

// Pack a KERB_INTERACTIVE_UNLOCK_LOGON into a contiguous buffer suitable
// for passing to LSA via GetSerialization. All UNICODE_STRING Buffer pointers
// become byte offsets from the start of the buffer.
HRESULT KerbInteractiveUnlockLogonPack(
    _In_ const KERB_INTERACTIVE_UNLOCK_LOGON& rkiul,
    _Out_ BYTE** prgb,
    _Out_ DWORD* pcb);

// Look up the Negotiate authentication package ID.
HRESULT LookupNegotiatePackageId(
    _Out_ ULONG* pulAuthPackage);

// Helper to initialize a UNICODE_STRING from a wide string (no allocation).
void UnicodeStringInitWithString(
    _In_ PWSTR pwz,
    _Out_ UNICODE_STRING* pus);

// Helper to pack a UNICODE_STRING into a contiguous buffer.
void UnicodeStringPackedUnicodeStringCopy(
    _In_ const UNICODE_STRING& rusIn,
    _In_ PWSTR pwzBuffer,
    _Out_ UNICODE_STRING* pusOut);
