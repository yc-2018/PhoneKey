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
static const wchar_t* kVerifyPipe = L"\\\\.\\pipe\\PhoneKeyVerify";

static int g_failures = 0;
static volatile LONG g_verifyNear = 1;
static HANDLE g_hVerifyStop = nullptr;
static HANDLE g_hVerifyThread = nullptr;

#define CHECK(cond, msg) do { \
    if (!(cond)) { printf("FAIL: %s\n", msg); ++g_failures; } \
    else { printf("ok: %s\n", msg); } \
} while (0)

static bool Contains(PCWSTR haystack, PCWSTR needle)
{
    return haystack != nullptr && wcsstr(haystack, needle) != nullptr;
}

// Minimal stand-in for the app's verification pipe. Requires the real
// PhoneKey app (with its own verify server) to be stopped.
static DWORD WINAPI VerifyServerProc(LPVOID)
{
    while (WaitForSingleObject(g_hVerifyStop, 0) != WAIT_OBJECT_0) {
        HANDLE h = CreateNamedPipeW(
            kVerifyPipe,
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
            1, 64, 64, 0, nullptr);
        if (h == INVALID_HANDLE_VALUE) {
            printf("mock verify pipe create failed: %lu\n", GetLastError());
            Sleep(100);
            continue;
        }
        BOOL connected = ConnectNamedPipe(h, nullptr);
        if (!connected && GetLastError() != ERROR_PIPE_CONNECTED) {
            CloseHandle(h);
            continue;
        }
        char buffer[64] = {};
        DWORD read = 0;
        if (ReadFile(h, buffer, sizeof(buffer) - 1, &read, nullptr)) {
            const char* response = "ERR";
            if (read >= 6 && strncmp(buffer, "VERIFY", 6) == 0) {
                bool bNear =
                    InterlockedCompareExchange(&g_verifyNear, 1, 1) != 0;
                response = bNear ? "OK" : "DENY";
            }
            DWORD written = 0;
            WriteFile(h, response, (DWORD)strlen(response), &written, nullptr);
            FlushFileBuffers(h);
        }
        DisconnectNamedPipe(h);
        CloseHandle(h);
    }
    return 0;
}

static void StartVerifyServer()
{
    g_hVerifyStop = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    g_hVerifyThread = CreateThread(
        nullptr, 0, VerifyServerProc, nullptr, 0, nullptr);
    Sleep(100);
}

static void StopVerifyServer()
{
    if (!g_hVerifyStop) return;
    SetEvent(g_hVerifyStop);
    // Unblock ConnectNamedPipe with a throwaway client.
    HANDLE h = CreateFileW(kVerifyPipe, GENERIC_READ | GENERIC_WRITE, 0,
        nullptr, OPEN_EXISTING, 0, nullptr);
    if (h != INVALID_HANDLE_VALUE) CloseHandle(h);
    if (g_hVerifyThread) {
        WaitForSingleObject(g_hVerifyThread, 3000);
        CloseHandle(g_hVerifyThread);
        g_hVerifyThread = nullptr;
    }
    CloseHandle(g_hVerifyStop);
    g_hVerifyStop = nullptr;
}

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
    StartVerifyServer();

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

    char statusReply[256] = {};
    pipeOk = SendPipe("STATUS:AWAY", statusReply, sizeof(statusReply));
    CHECK(pipeOk && strcmp(statusReply, "OK") == 0, "STATUS:AWAY accepted");
    PWSTR label = nullptr;
    hr = credential->GetStringValue(UFI_LABEL, &label);
    CHECK(SUCCEEDED(hr) && Contains(label, L"不在附近"), "away label shown");
    CoTaskMemFree(label);

    pipeOk = SendPipe("STATUS:NEAR", statusReply, sizeof(statusReply));
    CHECK(pipeOk && strcmp(statusReply, "OK") == 0, "STATUS:NEAR accepted");
    label = nullptr;
    hr = credential->GetStringValue(UFI_LABEL, &label);
    CHECK(SUCCEEDED(hr) && Contains(label, L"在附近"), "near label shown");
    CoTaskMemFree(label);

    autoLogon = TRUE;
    credential->SetSelected(&autoLogon);
    CHECK(autoLogon == FALSE, "lock-screen startup selection does not submit");

    Sleep(1700);

    // Click while the phone is away: the app's verify server says DENY,
    // so no credential may reach LSA and the pending offer stays for retry.
    InterlockedExchange(&g_verifyNear, 0);
    autoLogon = FALSE;
    credential->SetSelected(&autoLogon);
    CHECK(autoLogon == TRUE, "user click after grant attempts submit");

    response = (CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE)-1;
    statusText = nullptr;
    hr = credential->GetSerialization(&response, &serialization, &statusText, &statusIcon);
    CHECK(SUCCEEDED(hr), "GetSerialization with phone away returns S_OK");
    CHECK(response == CPGSR_NO_CREDENTIAL_NOT_FINISHED, "phone away denies serialization");
    CHECK(statusText != nullptr && Contains(statusText, L"不在附近"), "phone away warning text");
    CoTaskMemFree(statusText);

    // Pending survives the denial, so a retry works once the phone is near.
    InterlockedExchange(&g_verifyNear, 1);
    autoLogon = FALSE;
    credential->SetSelected(&autoLogon);
    CHECK(autoLogon == TRUE, "retry click submits after denial");

    response = (CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE)-1;
    statusText = nullptr;
    hr = credential->GetSerialization(&response, &serialization, &statusText, &statusIcon);
    CHECK(SUCCEEDED(hr), "GetSerialization with phone nearby returns S_OK");
    CHECK(response == CPGSR_RETURN_CREDENTIAL_FINISHED, "nearby click serializes credentials");
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
    StopVerifyServer();

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
