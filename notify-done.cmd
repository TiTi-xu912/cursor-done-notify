@echo off
chcp 65001 >nul
powershell.exe -NoProfile -NonInteractive -STA -ExecutionPolicy Bypass -File "%~dp0notify-done.ps1"
