"""Single source of truth for the application version.

Used by the updater (displayed in the window title and the update prompt) and
read by build.ps1 to set the Velopack package version. Bump this before each
release, then build and publish.
"""

VERSION = "1.0.2"
