#include <windows.h>
#include <string.h>
#include <strsafe.h>

#include "PresenceCheck.h"

// Keep in sync with app/presence_key/verify_pipe.py
static const WCHAR VERIFY_PIPE_NAME[] = L"\\\\.\\pipe\\PhoneKeyVerify";

static bool WriteCommand(HANDLE hPipe, const char* pszCommand)
{
    DWORD cbWritten = 0;
    size_t cbRemaining = strlen(pszCommand);
    const char* p = pszCommand;
    while (cbRemaining > 0) {
        if (!WriteFile(hPipe, p, (DWORD)cbRemaining, &cbWritten, nullptr) ||
            cbWritten == 0) {
            return false;
        }
        p += cbWritten;
        cbRemaining -= cbWritten;
    }
    return true;
}

bool IsPhoneNearby(DWORD timeoutMs)
{
    ULONGLONG deadline = GetTickCount64() + timeoutMs;
    HANDLE hPipe = INVALID_HANDLE_VALUE;

    for (;;) {
        hPipe = CreateFileW(
            VERIFY_PIPE_NAME,
            GENERIC_READ | GENERIC_WRITE,
            0,
            nullptr,
            OPEN_EXISTING,
            0,
            nullptr);
        if (hPipe != INVALID_HANDLE_VALUE) {
            break;
        }
        if (GetLastError() != ERROR_PIPE_BUSY) {
            // App not running or pipe not created yet: fail closed.
            return false;
        }
        ULONGLONG now = GetTickCount64();
        if (now >= deadline) {
            return false;
        }
        DWORD dwWait = (DWORD)(deadline - now);
        if (dwWait > 100) {
            dwWait = 100;
        }
        WaitNamedPipeW(VERIFY_PIPE_NAME, dwWait);
    }

    DWORD dwMode = PIPE_READMODE_MESSAGE;
    SetNamedPipeHandleState(hPipe, &dwMode, nullptr, nullptr);

    bool bNear = false;
    if (WriteCommand(hPipe, "VERIFY")) {
        FlushFileBuffers(hPipe);

        OVERLAPPED ov = {};
        ov.hEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        if (ov.hEvent) {
            char szResponse[32] = {};
            DWORD cbRead = 0;
            BOOL bRead = ReadFile(
                hPipe, szResponse, sizeof(szResponse) - 1, &cbRead, &ov);
            if (!bRead && GetLastError() == ERROR_IO_PENDING) {
                ULONGLONG now = GetTickCount64();
                DWORD dwWait = now < deadline ? (DWORD)(deadline - now) : 0;
                if (WaitForSingleObject(ov.hEvent, dwWait) == WAIT_OBJECT_0) {
                    bRead = GetOverlappedResult(hPipe, &ov, &cbRead, FALSE);
                } else {
                    CancelIo(hPipe);
                }
            }
            if (bRead) {
                szResponse[cbRead] = '\0';
                bNear = strncmp(szResponse, "OK", 2) == 0;
            }
            CloseHandle(ov.hEvent);
        }
    }

    CloseHandle(hPipe);
    return bNear;
}
