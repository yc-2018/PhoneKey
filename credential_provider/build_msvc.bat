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
    echo [INFO] Using !VCVARSALL!
    call "!VCVARSALL!" x64
    if errorlevel 1 exit /b 1
)

if not exist build mkdir build

echo [BUILD] Compiling credential provider...
cl.exe /nologo /W4 /WX- /EHsc /MT /O2 /utf-8 /DWIN32 /D_WINDOWS /DUNICODE /D_UNICODE ^
    /DNTDDI_VERSION=0x0A000000 /D_WIN32_WINNT=0x0A00 ^
    /c Dll.cpp ClassFactory.cpp UnlockProvider.cpp UnlockCredential.cpp ^
    PipeListener.cpp helpers.cpp /Fo:build\
if errorlevel 1 exit /b 1

echo [BUILD] Linking UnlockProvider.dll...
link.exe /nologo /DLL /OUT:build\UnlockProvider.dll /DEF:UnlockProvider.def ^
    build\Dll.obj build\ClassFactory.obj build\UnlockProvider.obj ^
    build\UnlockCredential.obj build\PipeListener.obj build\helpers.obj ^
    ole32.lib advapi32.lib secur32.lib user32.lib kernel32.lib ^
    uuid.lib shlwapi.lib
if errorlevel 1 exit /b 1

echo [OK] build\UnlockProvider.dll
