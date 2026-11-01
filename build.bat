@echo off
rem Rebuild AIUsageTracker with the current source and bundled template.
cd /d "%~dp0"
python -m pip install --quiet pyinstaller zstandard packaging
if errorlevel 1 exit /b 1
python -m PyInstaller --noconfirm --clean --onefile --noconsole --name AIUsageTracker --add-data "template.html;." --hidden-import zstandard widget.py
if errorlevel 1 exit /b 1
copy /y dist\AIUsageTracker.exe AIUsageTracker.exe >nul
if errorlevel 1 (
  echo Close the running AIUsageTracker and copy dist\AIUsageTracker.exe here.
  exit /b 1
)
