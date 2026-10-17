@echo off
setlocal
set "SCODEX_PYTHON="
for %%I in (python.exe) do set "SCODEX_PYTHON=%%~$PATH:I"
if not defined SCODEX_PYTHON exit /b 1
"%SCODEX_PYTHON%" "%~dp0scodex.py" %*
exit /b %errorlevel%
