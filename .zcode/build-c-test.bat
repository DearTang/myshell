@echo off
rem Build the FULL-config C TaskDialog repro with MSVC.
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
cd /d F:\workProject\personProject\myshell\.zcode
cl /W4 /utf-8 test-taskdialog-full.c /Fe:test-taskdialog-full.exe /link comctl32.lib ole32.lib user32.lib
