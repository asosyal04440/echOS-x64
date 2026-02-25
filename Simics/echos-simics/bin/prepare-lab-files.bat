@echo off
rem this file will be overwritten by the project setup script
setlocal
set SIMICS_BASE_PACKAGE=D:\Intel Simics Package Manager\simics-7.70.0
set SIMICS_PYTHON_PACKAGE=D:\Intel Simics Package Manager\simics-python-7.14.0\win64\bin\..\..
if "%SIMICS_PYTHON%"=="" (
    set "SIMICS_PYTHON="
)
"D:\Intel Simics Package Manager\simics-training-7.0.0-pre.12\bin\prepare-lab-files.bat"  %*
