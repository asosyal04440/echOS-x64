@echo off
rem ================================================================
rem  echOS Zero-Tolerance Gate Launcher
rem  Independent entry point — NOT overwritten by project-setup.bat
rem ================================================================
rem  Usage:
rem    bin\run-gate.bat                   (default: 600s timeout)
rem    bin\run-gate.bat --timeout 300     (custom timeout)
rem ================================================================

setlocal enabledelayedexpansion

set "GATE_ROOT=%~dp0.."
set "GATE_SCRIPT=%GATE_ROOT%\bin\zero-tolerance-gate.ps1"
set "TIMEOUT_SEC=600"

rem Parse --timeout argument
:parse_args
if "%~1"=="" goto :run
if /i "%~1"=="--timeout" (
    set "TIMEOUT_SEC=%~2"
    shift
    shift
    goto :parse_args
)
shift
goto :parse_args

:run
if not exist "%GATE_SCRIPT%" (
    echo [GATE] FATAL: Gate script not found: %GATE_SCRIPT%
    exit /b 2
)

echo [GATE] Launching Zero-Tolerance Gate (timeout=%TIMEOUT_SEC%s)
powershell -NoProfile -ExecutionPolicy Bypass -File "%GATE_SCRIPT%" -TimeoutSec %TIMEOUT_SEC%
exit /b %ERRORLEVEL%
