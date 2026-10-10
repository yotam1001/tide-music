# Security

Never include API keys, session cookies, access tokens or credential files in commits or issues. This prototype does not require an application API key and does not automatically read browser credentials.

Use GitHub's private vulnerability reporting where available for security issues. Avoid posting exploit details or secrets in public issues. If a credential is exposed, revoke it immediately; deleting a file alone does not remove it from Git history.

Only use trusted mpv and yt-dlp binaries. Local playback helpers run with your user permissions. Do not open untrusted executables disguised as media. Public service playback is unofficial and may fail as providers change.

Optional account imports grant access to your account. Keep the original header file outside Git and remove it when no longer needed. Tide saves an encrypted DPAPI copy in an owner-restricted directory, creates a temporary plaintext YouTube cookie lease for the owned player, and removes it after normal shutdown. Startup/disconnect reap leases from exited processes; conservative process checks can retain a lease after PID reuse or access denial. See [account setup](docs/ACCOUNT_SETUP.md) for these boundaries and disconnect behavior. Never attach account files to bug reports.
