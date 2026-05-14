@echo off
echo Building Azure DevOps Test Case Creator...
echo.
pyinstaller --onefile --windowed --name "DevOps Test Case Creator" main.py
echo.
echo Done. Check the 'dist' folder for the .exe file.
pause
