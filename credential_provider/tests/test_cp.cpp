#define INITGUID
#include <windows.h>
#include <credentialprovider.h>
#include <shlwapi.h>
#include <cstdio>
#include <cstring>

#include "guid.h"
#include "UnlockCredential.h"

typedef HRESULT (WINAPI* PFN_DllGetClassObject)(REFCLSID, REFIID, void**);
typedef HRESULT (WINAPI* PFN_DllCanUnloadNow)();

static const wchar_t* kPipe = L"\\\\.\\pipe\\CredentialProviderPipe";

static int g_failures = 0;

#define CHECK(cond, msg) do { \
    if (!(cond)) { printf("FAIL: %s\n", msg); ++g_failures; } \
    else { printf("ok: %s\n", msg); } \
} while (0)

static bool SendPipe(const char* cmd, char* response, DWORD responseSize)
{
    HANDLE h = INVALID_HANDLE_VALUE;
    for (int i = 0; i < 40; ++i) {
        h = CreateFileW(kPipe, GENERIC_READ | GENERIC_WRITE, 0, nullptr,
            OPEN_EXISTING, 0, nullptr);
        if (h != INVALID_HANDLE_VALUE) break;
        Sleep(100);
    }
    if (h == INVALID_HANDLE_VALUE) {
        printf("pipe connect failed: %lu\n", GetLastError());
        return false;
    }

    DWORD mode = PIPE_READMODE_MESSAGE;
    SetNamedPipeHandleState(h, &mode, nullptr, nullptr);

    DWORD written = 0;
    BOOL ok = WriteFile(h, cmd, (DWORD)strlen(cmd), &written, nullptr);
    if (ok) FlushFileBuffers(h);

    DWORD read = 0;
    if (ok) {
        Sleep(50);
        ok = ReadFile(h, response, responseSize - 1, &read, nullptr);
    }
    CloseHandle(h);
    if (!ok) {
        printf("pipe io failed: %lu\n", GetLastError());
        return false;
    }
    response[read] = '\0';
    return true;
}

int main()
{
    setvbuf(stdout, nullptr, _IONBF, 0);
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

    HMODULE module = LoadLibraryW(L"UnlockProvider.dll");
    if (!module) {
        printf("LoadLibrary failed: %lu\n", GetLastError());
        return 1;
    }

    auto getClassObject = (PFN_DllGetClassObject)GetProcAddress(module, "DllGetClassObject");
    auto canUnloadNow = (PFN_DllCanUnloadNow)GetProcAddress(module, "DllCanUnloadNow");
    CHECK(getClassObject != nullptr, "DllGetClassObject exported");
    CHECK(canUnloadNow != nullptr, "DllCanUnloadNow exported");
    if (!getClassObject || !canUnloadNow) return 1;

    IClassFactory* factory = nullptr;
    HRESULT hr = getClassObject(CLSID_UnlockProvider, IID_IClassFactory, (void**)&factory);
    CHECK(SUCCEEDED(hr) && factory != nullptr, "class factory created");
    if (!factory) return 1;

    ICredentialProvider* provider = nullptr;
    hr = factory->CreateInstance(nullptr, IID_ICredentialProvider, (void**)&provider);
    CHECK(SUCCEEDED(hr) && provider != nullptr, "provider created");
    if (!provider) return 1;

    hr = provider->SetUsageScenario(CPUS_UNLOCK_WORKSTATION, 0);
    CHECK(SUCCEEDED(hr), "usage scenario accepted");

    DWORD count = 0;
    DWORD defaultValue = 0;
    BOOL autoLogonDefault = TRUE;
    hr = provider->GetCredentialCount(&count, &defaultValue, &autoLogonDefault);
    CHECK(SUCCEEDED(hr), "GetCredentialCount");
    CHECK(count == 1, "exactly one tile");
    CHECK(defaultValue == CREDENTIAL_PROVIDER_NO_DEFAULT, "no default credential");
    CHECK(autoLogonDefault == FALSE, "never auto-logon on enumeration");

    ICredentialProviderCredential* credential = nullptr;
    hr = provider->GetCredentialAt(0, &credential);
    CHECK(SUCCEEDED(hr) && credential != nullptr, "credential obtained");
    if (!credential) return 1;

    hr = credential->Advise(nullptr);
    CHECK(SUCCEEDED(hr), "credential advised");

    CREDENTIAL_PROVIDER_FIELD_STATE fieldState = CPFS_DISPLAY_IN_BOTH;
    CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE fieldInteractive = CPFIS_NONE;
    hr = credential->GetFieldState(UFI_USERNAME, &fieldState, &fieldInteractive);
    CHECK(SUCCEEDED(hr) && fieldState == CPFS_HIDDEN, "username field is hidden");
    hr = credential->GetFieldState(UFI_PASSWORD, &fieldState, &fieldInteractive);
    CHECK(SUCCEEDED(hr) && fieldState == CPFS_HIDDEN, "password field is hidden");
    DWORD adjacent = 99;
    hr = credential->GetSubmitButtonValue(UFI_SUBMIT, &adjacent);
    CHECK(SUCCEEDED(hr) && adjacent == UFI_LABEL, "submit arrow sits beside the label");

    BOOL autoLogon = TRUE;
    hr = credential->SetSelected(&autoLogon);
    CHECK(SUCCEEDED(hr), "SetSelected");
    CHECK(autoLogon == FALSE, "selecting the tile does not submit");

    CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE response = (CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE)-1;
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION serialization = {};
    PWSTR statusText = nullptr;
    CREDENTIAL_PROVIDER_STATUS_ICON statusIcon = CPSI_NONE;

    hr = credential->GetSerialization(&response, &serialization, &statusText, &statusIcon);
    CHECK(SUCCEEDED(hr), "GetSerialization without grant returns S_OK");
    CHECK(response == CPGSR_NO_CREDENTIAL_NOT_FINISHED, "no grant -> no credential");
    CHECK(statusText != nullptr, "no grant -> warning text");
    CoTaskMemFree(statusText);

    char reply[256] = {};
    bool pipeOk = SendPipe("UNLOCK:.\\test:pw", reply, sizeof(reply));
    CHECK(pipeOk && strcmp(reply, "OK") == 0, "UNLOCK grant accepted");

    autoLogon = TRUE;
    credential->SetSelected(&autoLogon);
    CHECK(autoLogon == FALSE, "lock-screen startup selection does not submit");

    Sleep(1700);

    autoLogon = FALSE;
    credential->SetSelected(&autoLogon);
    CHECK(autoLogon == TRUE, "user click after grant unlocks directly");

    response = (CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE)-1;
    statusText = nullptr;
    hr = credential->GetSerialization(&response, &serialization, &statusText, &statusIcon);
    CHECK(SUCCEEDED(hr), "GetSerialization with grant returns S_OK");
    CHECK(response == CPGSR_RETURN_CREDENTIAL_FINISHED, "click after grant serializes credentials");
    CHECK(serialization.cbSerialization > 0, "serialization buffer present");
    CoTaskMemFree(statusText);
    if (serialization.rgbSerialization) CoTaskMemFree(serialization.rgbSerialization);
    ZeroMemory(&serialization, sizeof(serialization));

    char reply2[256] = {};
    pipeOk = SendPipe("CLEAR", reply2, sizeof(reply2));
    CHECK(pipeOk && strcmp(reply2, "OK") == 0, "CLEAR accepted");

    autoLogon = TRUE;
    credential->SetSelected(&autoLogon);
    CHECK(autoLogon == FALSE, "CLEAR also disables direct unlock");

    response = (CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE)-1;
    statusText = nullptr;
    hr = credential->GetSerialization(&response, &serialization, &statusText, &statusIcon);
    CHECK(SUCCEEDED(hr), "GetSerialization after CLEAR returns S_OK");
    CHECK(response == CPGSR_NO_CREDENTIAL_NOT_FINISHED, "CLEAR revokes the grant");
    CoTaskMemFree(statusText);

    credential->Release();
    provider->Release();
    factory->Release();

    hr = canUnloadNow();
    CHECK(hr == S_OK, "DllCanUnloadNow reports unloadable");

    // TDM-GCC's static winpthread exit hooks misbehave when a MinGW DLL has
    // been unloaded; skip CRT teardown for this harness.
    if (g_failures == 0) {
        printf("ALL CHECKS PASSED\n");
    } else {
        printf("%d CHECKS FAILED\n", g_failures);
    }
    ExitProcess(g_failures == 0 ? 0 : 1);
}
