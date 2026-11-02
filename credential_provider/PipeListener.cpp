#include <windows.h>
#include <sddl.h>
#include <strsafe.h>

#include "PipeListener.h"
#include "UnlockProvider.h"

#define PIPE_NAME L"\\\\.\\pipe\\CredentialProviderPipe"
#define PIPE_BUFFER_SIZE 512
#define PIPE_TIMEOUT_MS  5000

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

CPipeListener::CPipeListener() :
    _pProvider(nullptr),
    _hThread(nullptr),
    _hStopEvent(nullptr)
{
}

CPipeListener::~CPipeListener()
{
    Stop();
}

HRESULT CPipeListener::Start(_In_ CUnlockProvider* pProvider)
{
    if (_hThread) return S_OK; // Already running

    _pProvider = pProvider;

    _hStopEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    if (!_hStopEvent) return HRESULT_FROM_WIN32(GetLastError());

    _hThread = CreateThread(nullptr, 0, s_ThreadProc, this, 0, nullptr);
    if (!_hThread) {
        DWORD dwErr = GetLastError();
        CloseHandle(_hStopEvent);
        _hStopEvent = nullptr;
        return HRESULT_FROM_WIN32(dwErr);
    }

    DbgLog(L"[PipeListener] Started\n");
    return S_OK;
}

void CPipeListener::Stop()
{
    if (_hStopEvent) {
        SetEvent(_hStopEvent);
    }
    if (_hThread) {
        WaitForSingleObject(_hThread, 5000);
        CloseHandle(_hThread);
        _hThread = nullptr;
    }
    if (_hStopEvent) {
        CloseHandle(_hStopEvent);
        _hStopEvent = nullptr;
    }
    _pProvider = nullptr;
    DbgLog(L"[PipeListener] Stopped\n");
}

DWORD WINAPI CPipeListener::s_ThreadProc(_In_ LPVOID lpParameter)
{
    CPipeListener* pThis = static_cast<CPipeListener*>(lpParameter);
    return pThis->_ThreadProc();
}

DWORD CPipeListener::_ThreadProc()
{
    DbgLog(L"[PipeListener] Thread started\n");

    // Build security descriptor for the pipe:
    // SYSTEM: full access, Administrators: full access,
    // Authenticated Users: read+write, deny Anonymous, deny Network
    PSECURITY_DESCRIPTOR pSD = nullptr;
    BOOL bSuccess = ConvertStringSecurityDescriptorToSecurityDescriptorW(
        L"D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;AU)(D;;GA;;;AN)(D;;GA;;;NU)",
        SDDL_REVISION_1,
        &pSD,
        nullptr);

    if (!bSuccess) {
        DbgLog(L"[PipeListener] ERROR: ConvertStringSD failed: %lu\n", GetLastError());
        return GetLastError();
    }

    SECURITY_ATTRIBUTES sa = {};
    sa.nLength = sizeof(sa);
    sa.lpSecurityDescriptor = pSD;
    sa.bInheritHandle = FALSE;

    while (WaitForSingleObject(_hStopEvent, 0) != WAIT_OBJECT_0) {
        // Create the named pipe instance with anti-squatting protection
        HANDLE hPipe = CreateNamedPipeW(
            PIPE_NAME,
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,                  // Max instances
            PIPE_BUFFER_SIZE,   // Out buffer
            PIPE_BUFFER_SIZE,   // In buffer
            0,                  // Default timeout
            &sa);

        if (hPipe == INVALID_HANDLE_VALUE) {
            DWORD dwErr = GetLastError();
            DbgLog(L"[PipeListener] CreateNamedPipe failed: %lu, retrying in 1s\n", dwErr);
            // Wait 1 second or until stop event
            if (WaitForSingleObject(_hStopEvent, 1000) == WAIT_OBJECT_0) break;
            continue;
        }

        DbgLog(L"[PipeListener] Pipe created, waiting for client...\n");

        // Wait for a client connection using overlapped I/O
        OVERLAPPED ov = {};
        ov.hEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        if (!ov.hEvent) {
            CloseHandle(hPipe);
            break;
        }

        BOOL bConnected = ConnectNamedPipe(hPipe, &ov);
        if (!bConnected) {
            DWORD dwErr = GetLastError();
            if (dwErr == ERROR_IO_PENDING) {
                // Wait for either client connection or stop event
                HANDLE hEvents[2] = { ov.hEvent, _hStopEvent };
                DWORD dwWait = WaitForMultipleObjects(2, hEvents, FALSE, INFINITE);

                if (dwWait != WAIT_OBJECT_0) {
                    // Stop event signaled or error
                    CancelIo(hPipe);
                    CloseHandle(ov.hEvent);
                    CloseHandle(hPipe);
                    break;
                }
            } else if (dwErr != ERROR_PIPE_CONNECTED) {
                DbgLog(L"[PipeListener] ConnectNamedPipe error: %lu\n", dwErr);
                CloseHandle(ov.hEvent);
                CloseHandle(hPipe);
                continue;
            }
            // ERROR_PIPE_CONNECTED means client connected before we called ConnectNamedPipe - OK
        }
        CloseHandle(ov.hEvent);

        DbgLog(L"[PipeListener] Client connected\n");

        // Client connected - read command with timeout
        char szBuffer[PIPE_BUFFER_SIZE] = {};
        DWORD cbRead = 0;

        OVERLAPPED ovRead = {};
        ovRead.hEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        if (!ovRead.hEvent) {
            DisconnectNamedPipe(hPipe);
            CloseHandle(hPipe);
            continue;
        }

        BOOL bRead = ReadFile(hPipe, szBuffer, sizeof(szBuffer) - 1, &cbRead, &ovRead);
        if (!bRead && GetLastError() == ERROR_IO_PENDING) {
            HANDLE hEvents[2] = { ovRead.hEvent, _hStopEvent };
            DWORD dwWait = WaitForMultipleObjects(2, hEvents, FALSE, PIPE_TIMEOUT_MS);
            if (dwWait == WAIT_OBJECT_0) {
                GetOverlappedResult(hPipe, &ovRead, &cbRead, FALSE);
            } else {
                CancelIo(hPipe);
                CloseHandle(ovRead.hEvent);
                DisconnectNamedPipe(hPipe);
                CloseHandle(hPipe);
                if (dwWait == WAIT_OBJECT_0 + 1) break; // Stop event
                DbgLog(L"[PipeListener] Read timeout\n");
                continue; // Timeout
            }
        }
        CloseHandle(ovRead.hEvent);

        szBuffer[cbRead] = '\0';
        DbgLog(L"[PipeListener] Received %lu bytes\n", cbRead);

        // Parse the command
        char szResponse[64] = "ERR:INVALID_CMD";

        // Clear pending credentials when the phone is no longer nearby.
        if (cbRead >= 5 && strncmp(szBuffer, "CLEAR", 5) == 0) {
            if (_pProvider) {
                HRESULT hr = _pProvider->OnClearCommand();
                StringCchCopyA(
                    szResponse,
                    ARRAYSIZE(szResponse),
                    SUCCEEDED(hr) ? "OK" : "ERR:PROVIDER_FAILED");
                DbgLog(L"[PipeListener] Clear pending credentials: hr=0x%08X\n", hr);
            } else {
                StringCchCopyA(szResponse, ARRAYSIZE(szResponse), "ERR:NO_PROVIDER");
            }
        }
        // Expected format: UNLOCK:domain\username:password
        // or: UNLOCK:username:password (domain defaults to ".")
        else if (cbRead > 7 && strncmp(szBuffer, "UNLOCK:", 7) == 0) {
            char* pPayload = szBuffer + 7;

            // Find the separator between user-part and password
            char* pColon = nullptr;
            char* pBackslash = strchr(pPayload, '\\');
            if (pBackslash) {
                pColon = strchr(pBackslash + 1, ':');
            } else {
                pColon = strchr(pPayload, ':');
            }

            if (pColon) {
                *pColon = '\0';
                char* pPassword = pColon + 1;
                char* pUserPart = pPayload;

                WCHAR szDomain[256] = L".";
                WCHAR szUsername[256] = {};
                WCHAR szPasswordW[256] = {};

                // Parse domain\username
                if (pBackslash) {
                    *pBackslash = '\0';
                    MultiByteToWideChar(CP_UTF8, 0, pUserPart, -1, szDomain, ARRAYSIZE(szDomain));
                    MultiByteToWideChar(CP_UTF8, 0, pBackslash + 1, -1, szUsername, ARRAYSIZE(szUsername));
                } else {
                    MultiByteToWideChar(CP_UTF8, 0, pUserPart, -1, szUsername, ARRAYSIZE(szUsername));
                }

                MultiByteToWideChar(CP_UTF8, 0, pPassword, -1, szPasswordW, ARRAYSIZE(szPasswordW));

                DbgLog(L"[PipeListener] Parsed: domain=%s user=%s\n", szDomain, szUsername);

                // Pass to provider
                if (_pProvider) {
                    HRESULT hr = _pProvider->OnUnlockCommand(szDomain, szUsername, szPasswordW);
                    if (SUCCEEDED(hr)) {
                        StringCchCopyA(szResponse, ARRAYSIZE(szResponse), "OK");
                        DbgLog(L"[PipeListener] OnUnlockCommand succeeded\n");
                    } else {
                        StringCchCopyA(szResponse, ARRAYSIZE(szResponse), "ERR:PROVIDER_FAILED");
                        DbgLog(L"[PipeListener] OnUnlockCommand failed: 0x%08X\n", hr);
                    }
                } else {
                    StringCchCopyA(szResponse, ARRAYSIZE(szResponse), "ERR:NO_PROVIDER");
                    DbgLog(L"[PipeListener] ERROR: No provider reference\n");
                }

                // Securely clear password from stack
                SecureZeroMemory(szPasswordW, sizeof(szPasswordW));
                SecureZeroMemory(pPassword, strlen(pPassword));
            }
        } else {
            DbgLog(L"[PipeListener] Invalid command\n");
        }

        // Send response
        DWORD cbWritten = 0;
        WriteFile(hPipe, szResponse, (DWORD)strlen(szResponse), &cbWritten, nullptr);
        FlushFileBuffers(hPipe);

        DbgLog(L"[PipeListener] Sent response: %S\n", szResponse);

        DisconnectNamedPipe(hPipe);
        CloseHandle(hPipe);
    }

    LocalFree(pSD);
    DbgLog(L"[PipeListener] Thread exiting\n");
    return 0;
}
