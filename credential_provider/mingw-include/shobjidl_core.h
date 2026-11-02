#pragma once

// MinGW-w64 has no shobjidl_core.h; the Windows SDK credentialprovider.h
// includes it only for the shell interfaces, which this project never uses.
#include <shobjidl.h>
