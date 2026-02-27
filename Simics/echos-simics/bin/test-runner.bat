
@echo off
rem this file will be overwritten by the project setup script
setlocal
set SIMICS_BASE_PACKAGE=D:\Intel Simics Package Manager\simics-7.70.0
set SIMICS_PYTHON_PACKAGE=D:\Intel Simics Package Manager\simics-python-7.14.0\win64\bin\..\..
if "%SIMICS_PYTHON%"=="" (
    set "SIMICS_PYTHON="
)
if exist "C:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\Simics\echos-simics\.package-list" set SIMICS_EPL=--package-list "C:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\Simics\echos-simics\.package-list"
if not exist "C:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\Simics\echos-simics\.package-list" set SIMICS_EPL=
if /I "%~1"=="--zero-tolerance-gate" (
    powershell -NoProfile -ExecutionPolicy Bypass -File "C:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\Simics\echos-simics\bin\zero-tolerance-gate.ps1"
    exit /b %ERRORLEVEL%
)
"D:\Intel Simics Package Manager\simics-7.70.0\bin\test-runner.bat" %SIMICS_EPL% --project "C:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\Simics\echos-simics" %*
