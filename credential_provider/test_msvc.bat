@echo off
setlocal enabledelayedexpansion
cd /d "%~dp0"

where cl.exe >nul 2>nul
if errorlevel 1 (
    set "VCVARSALL="
    for %%P in (
        "%ProgramFiles%\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvarsall.bat"
        "%ProgramFiles%\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvarsall.bat"
        "%ProgramFiles%\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat"
        "%ProgramFiles%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"
        "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"
    ) do (
        if exist "%%~P" set "VCVARSALL=%%~P"
    )
    if not defined VCVARSALL (
        for /f "usebackq tokens=*" %%I in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do (
            if exist "%%I\VC\Auxiliary\Build\vcvarsall.bat" set "VCVARSALL=%%I\VC\Auxiliary\Build\vcvarsall.bat"
        )
    )
    if not defined VCVARSALL (
        echo [ERROR] Visual Studio C++ build tools not found.
        exit /b 1
    )
    call "!VCVARSALL!" x64
    if errorlevel 1 exit /b 1
)

if not exist build\UnlockProvider.dll (
    echo [ERROR] build\UnlockProvider.dll missing; run build_msvc.bat first.
    exit /b 1
)

echo [TEST] Compiling test harness...
cl.exe /nologo /W4 /WX- /EHsc /MT /O2 /utf-8 /DWIN32 /D_WINDOWS /DUNICODE /D_UNICODE ^
    tests\test_cp.cpp /I. /Fo:build\ /Fe:build\test_cp.exe ^
    /link ole32.lib advapi32.lib user32.lib kernel32.lib shlwapi.lib
if errorlevel 1 exit /b 1

echo [TEST] Running credential provider checks...
pushd build
test_cp.exe
set "TEST_EXIT=%ERRORLEVEL%"
popd
exit /b %TEST_EXIT%
