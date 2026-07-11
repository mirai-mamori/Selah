@echo off
setlocal

rem Double-click-friendly development launcher. The bypass is process-scoped
rem and does not change the user's PowerShell execution policy.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0run.ps1" %*
set "exitCode=%ERRORLEVEL%"

if not "%exitCode%"=="0" (
    echo.
    echo Selah development command failed with exit code %exitCode%.
    pause
)

exit /b %exitCode%
