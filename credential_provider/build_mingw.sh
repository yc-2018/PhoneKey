#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

CXX="${CXX:-g++}"
SDK_VERSION="10.0.16299.0"

mkdir -p build sdkinc
if [[ ! -f sdkinc/credentialprovider.h ]]; then
    echo "[INFO] Downloading Windows SDK ${SDK_VERSION} credentialprovider.h"
    curl -fsSL \
        "https://raw.githubusercontent.com/tpn/winsdk-10/master/Include/${SDK_VERSION}/um/credentialprovider.h" \
        -o sdkinc/credentialprovider.h
fi

COMMON=(
    -std=c++17 -O2 -Wall -Wno-unknown-pragmas -Wno-write-strings
    -Wno-delete-non-virtual-dtor -DUNICODE -D_UNICODE -DWIN32 -D_WINDOWS
    -DNTDDI_VERSION=0x0A000000 -D_WIN32_WINNT=0x0A00
    -I. -Isdkinc -Imingw-include
)
LIBS=(-lole32 -ladvapi32 -lsecur32 -lshlwapi -luser32 -lkernel32 -luuid)

for src in Dll.cpp ClassFactory.cpp UnlockProvider.cpp UnlockCredential.cpp \
           PipeListener.cpp PresenceCheck.cpp helpers.cpp mingw/cp_guids.cpp; do
    obj="build/$(basename "${src%.cpp}").o"
    echo "[BUILD] ${src}"
    "$CXX" "${COMMON[@]}" -c "$src" -o "$obj"
done

echo "[LINK] build/UnlockProvider.dll"
"$CXX" -static -static-libgcc -static-libstdc++ -shared \
    -o build/UnlockProvider.dll build/*.o \
    -Wl,UnlockProvider.def "${LIBS[@]}"

echo "[TEST] build/test_cp.exe"
"$CXX" -static -static-libgcc -static-libstdc++ "${COMMON[@]}" \
    tests/test_cp.cpp mingw/cp_guids.cpp -o build/test_cp.exe "${LIBS[@]}"

cd build
./test_cp.exe
