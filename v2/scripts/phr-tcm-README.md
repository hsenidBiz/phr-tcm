# Test Case Manager - releases

This repository holds the installer and update packages for the Test Case
Manager desktop app. The app checks here for updates on its own; you only
need this page to install it the first time.

## Install

1. Open `AzureDevOpsTestCaseManager.V2-win-Setup.exe` in the file list
   above and use **Download**.
2. Run it. The app installs for your user and starts.

From then on the app updates itself: when a new version is published here
it shows a **Restart to update** banner.

## No access?

If the app tells you it cannot reach this repository, raise a Redmine
ticket asking for read access to `HRM / PHR-TCM`.

*The files on this branch are replaced on every release; only the
newest five versions are kept. Do not commit here by hand.*
