@echo off
node "%~dp0test-local.mjs" %*
exit /b %errorlevel%
