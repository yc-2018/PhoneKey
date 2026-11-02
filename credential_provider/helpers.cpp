#include "helpers.h"
#include <intsafe.h>

#define NEGOSSP_NAME_A  "Negotiate"

void UnicodeStringInitWithString(
    _In_ PWSTR pwz,
    _Out_ UNICODE_STRING* pus)
{
    USHORT lenBytes = (USHORT)(wcslen(pwz) * sizeof(WCHAR));
    pus->Buffer = pwz;
    pus->Length = lenBytes;
    pus->MaximumLength = lenBytes;
}

void UnicodeStringPackedUnicodeStringCopy(
    _In_ const UNICODE_STRING& rusIn,
    _In_ PWSTR pwzBuffer,
    _Out_ UNICODE_STRING* pusOut)
{
    pusOut->Length = rusIn.Length;
    pusOut->MaximumLength = rusIn.Length;
    pusOut->Buffer = pwzBuffer;

    CopyMemory(pwzBuffer, rusIn.Buffer, rusIn.Length);
}

HRESULT KerbInteractiveUnlockLogonInit(
    _In_ PWSTR pszDomain,
    _In_ PWSTR pszUsername,
    _In_ PWSTR pszPassword,
    _In_ CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus,
    _Out_ KERB_INTERACTIVE_UNLOCK_LOGON* pkiul)
{
    ZeroMemory(pkiul, sizeof(*pkiul));

    KERB_INTERACTIVE_LOGON* pkil = &pkiul->Logon;

    // Set message type based on usage scenario
    switch (cpus) {
    case CPUS_UNLOCK_WORKSTATION:
        pkil->MessageType = KerbWorkstationUnlockLogon;
        break;
    case CPUS_LOGON:
    case CPUS_CREDUI:
        pkil->MessageType = KerbInteractiveLogon;
        break;
    default:
        return E_INVALIDARG;
    }

    UnicodeStringInitWithString(pszDomain, &pkil->LogonDomainName);
    UnicodeStringInitWithString(pszUsername, &pkil->UserName);
    UnicodeStringInitWithString(pszPassword, &pkil->Password);

    return S_OK;
}

HRESULT KerbInteractiveUnlockLogonPack(
    _In_ const KERB_INTERACTIVE_UNLOCK_LOGON& rkiul,
    _Out_ BYTE** prgb,
    _Out_ DWORD* pcb)
{
    *prgb = nullptr;
    *pcb = 0;

    const KERB_INTERACTIVE_LOGON* pkil = &rkiul.Logon;

    // Calculate total buffer size
    DWORD cbStruct = sizeof(KERB_INTERACTIVE_UNLOCK_LOGON);
    DWORD cbStrings = pkil->LogonDomainName.Length +
                      pkil->UserName.Length +
                      pkil->Password.Length;
    DWORD cbTotal = cbStruct + cbStrings;

    BYTE* rgb = (BYTE*)CoTaskMemAlloc(cbTotal);
    if (!rgb) {
        return E_OUTOFMEMORY;
    }

    // Copy the struct
    CopyMemory(rgb, &rkiul, cbStruct);

    // Get pointers into the packed buffer
    KERB_INTERACTIVE_UNLOCK_LOGON* pkiulPacked =
        (KERB_INTERACTIVE_UNLOCK_LOGON*)rgb;
    KERB_INTERACTIVE_LOGON* pkilPacked = &pkiulPacked->Logon;

    // Pack strings after the struct
    BYTE* pbStrings = rgb + cbStruct;

    // Pack LogonDomainName
    pkilPacked->LogonDomainName.Length = pkil->LogonDomainName.Length;
    pkilPacked->LogonDomainName.MaximumLength = pkil->LogonDomainName.Length;
    pkilPacked->LogonDomainName.Buffer = (PWSTR)(pbStrings - rgb);
    CopyMemory(pbStrings, pkil->LogonDomainName.Buffer, pkil->LogonDomainName.Length);
    pbStrings += pkil->LogonDomainName.Length;

    // Pack UserName
    pkilPacked->UserName.Length = pkil->UserName.Length;
    pkilPacked->UserName.MaximumLength = pkil->UserName.Length;
    pkilPacked->UserName.Buffer = (PWSTR)(pbStrings - rgb);
    CopyMemory(pbStrings, pkil->UserName.Buffer, pkil->UserName.Length);
    pbStrings += pkil->UserName.Length;

    // Pack Password
    pkilPacked->Password.Length = pkil->Password.Length;
    pkilPacked->Password.MaximumLength = pkil->Password.Length;
    pkilPacked->Password.Buffer = (PWSTR)(pbStrings - rgb);
    CopyMemory(pbStrings, pkil->Password.Buffer, pkil->Password.Length);

    *prgb = rgb;
    *pcb = cbTotal;

    return S_OK;
}

HRESULT LookupNegotiatePackageId(
    _Out_ ULONG* pulAuthPackage)
{
    *pulAuthPackage = 0;

    HANDLE hLsa = nullptr;
    NTSTATUS status = LsaConnectUntrusted(&hLsa);
    if (FAILED(HRESULT_FROM_NT(status))) {
        return HRESULT_FROM_NT(status);
    }

    LSA_STRING lsaszPackageName;
    lsaszPackageName.Buffer = (PCHAR)NEGOSSP_NAME_A;
    lsaszPackageName.Length = (USHORT)strlen(NEGOSSP_NAME_A);
    lsaszPackageName.MaximumLength = lsaszPackageName.Length + 1;

    ULONG ulPackage = 0;
    status = LsaLookupAuthenticationPackage(hLsa, &lsaszPackageName, &ulPackage);
    LsaDeregisterLogonProcess(hLsa);

    if (FAILED(HRESULT_FROM_NT(status))) {
        return HRESULT_FROM_NT(status);
    }

    *pulAuthPackage = ulPackage;
    return S_OK;
}
