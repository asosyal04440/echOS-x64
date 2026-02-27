@echo off
REM ============================================
REM  echOS GUI Boot Launcher
REM  Simics penceresinde echOS ekranini gorebilirsin
REM ============================================

cd /d "%~dp0\.."

echo.
echo  echOS GUI Boot baslatiliyor...
echo  Simics GUI acilinca "Run" butonuna bas veya konsola "run" yaz.
echo  VGA penceresi echOS ekranini gosterecek.
echo.

call simics.bat targets\echos\gui-boot.simics
