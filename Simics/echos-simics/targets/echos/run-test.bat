@echo off
REM echOS Simics Test Launcher
REM
REM This script launches Simics with echOS test configuration

echo.
echo   echOS Simics Test Launcher
echo   ==========================
echo.

cd /d "%~dp0"

REM Check if ech_os.efi exists
if not exist "images\bootx64.efi" (
    echo ERROR: echOS binary not found!
    echo Please build echOS first: cargo build --target x86_64-unknown-uefi
    echo Then copy: target\x86_64-unknown-uefi\debug\ech_os.efi to images\bootx64.efi
    pause
    exit /b 1
)

echo Starting Simics...
echo.

REM Launch Simics with echOS configuration
call ..\bin\simics.exe -no-gui -e "run-script %%simics%%/targets/echos/tests/boot-test.simics" -e "quit"

echo.
echo Test completed.
pause
