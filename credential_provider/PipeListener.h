#pragma once

#include <windows.h>

// Forward declaration
class CUnlockProvider;

// Pipe listener that runs in a dedicated thread.
// Listens for UNLOCK commands on a named pipe and triggers
// auto-logon through the credential provider.
class CPipeListener {
public:
    CPipeListener();
    ~CPipeListener();

    // Start the listener thread. pProvider must remain valid
    // until Stop() is called and completes.
    HRESULT Start(_In_ CUnlockProvider* pProvider);

    // Signal the listener thread to stop and wait for it to exit.
    void Stop();

private:
    static DWORD WINAPI s_ThreadProc(_In_ LPVOID lpParameter);
    DWORD _ThreadProc();

    CUnlockProvider*    _pProvider;
    HANDLE              _hThread;
    HANDLE              _hStopEvent;
};
