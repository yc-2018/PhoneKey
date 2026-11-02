#pragma once

#include <windows.h>

// Ask the PhoneKey tray app to verify that the bound phone is currently
// nearby. Runs synchronously and fails closed: any pipe error or timeout
// is treated as "not nearby".
bool IsPhoneNearby(DWORD timeoutMs);
