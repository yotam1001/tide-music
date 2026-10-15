# Chrome Web Store submission checklist

This is a preparation checklist, not a submitted or approved listing. The current
companion is an unpacked prototype with a separately installed Windows native
helper. A maintainer must review the final package and current Chrome policies
before submitting it.

## Product purpose and data handling

- [ ] Use an accurate single-purpose description: **Explicitly transfer the
  session from the selected YouTube Music tab to the local Tide Windows player
  so it can verify the account and access its music library and playback.**
- [ ] Describe the Windows x64 player/native helper prerequisite, explicit import,
  selected Music account and exact extension-ID registration. Do not imply the
  extension plays music by itself, installs the helper, provides official Google
  OAuth, or is affiliated with Google/YouTube.
- [ ] Audit the exact uploaded manifest and code, including generated package
  contents. Explain `activeTab`, `webRequest`, `nativeMessaging` and the sole
  `https://music.youtube.com/*` host permission. Request no broader future-use
  permission. Review the [minimum-permission guidance](https://developer.chrome.com/docs/webstore/program-policies/user-data-faq).
- [ ] Complete the dashboard's single-purpose, permission justification and data
  handling fields from the actual implementation. Authentication cookies,
  account/visitor identifiers and request metadata are handled even though the
  extension transfers them locally. Do not select a blanket “no user data” answer.
  Consult [privacy-field instructions](https://developer.chrome.com/docs/webstore/cws-dashboard-privacy).
- [ ] Review whether authentication information, identifiers, web activity and
  other dashboard categories apply to this exact package. Local-only processing
  still needs disclosure; assess the linked native component too. Have the human
  publisher verify each certification rather than automatically checking boxes.
- [ ] Ensure popup disclosure appears **before** import and accurately states
  that session cookies/account information go to the local helper for account
  verification and use by Tide. The import action must express consent. A policy
  link alone does not replace disclosure in the product. See
  [Chrome's user-data FAQ](https://developer.chrome.com/docs/webstore/program-policies/user-data-faq).

## Public privacy notice

- [ ] Review [PRIVACY.md](../PRIVACY.md) against the final extension, player and
  installer. It explains local session transfer, encrypted persistent storage,
  plaintext playback leases, native Google/YouTube traffic, public downloads,
  retention and removal. Remove unsupported promises from the listing.
- [ ] Merge the notice and provide a public HTTPS privacy-policy URL accessible
  without sign-in. Candidate after merge:
  `https://github.com/yotam1001/tide-music/blob/main/PRIVACY.md`.
  Verify the actual page exists before entering it in the dashboard or popup.
- [ ] Keep dashboard declarations, listing, in-product disclosure and policy
  consistent. Clearly limit data use and transfer to the stated function; do not
  use session or browsing data for advertising, sale or unrelated profiling.
- [ ] Review the security and Limited Use requirements with the publisher.
  Explain the local native channel and temporary plaintext files honestly;
  do not claim blanket compliance or that all data is always encrypted.
  [Chrome's user-data FAQ](https://developer.chrome.com/docs/webstore/program-policies/user-data-faq)
  is guidance, not an approval of this implementation.

## Package, listing and reviewer access

- [ ] Choose a final public name/version and include suitable extension icons,
  listing screenshots and any required promotional assets. Check current upload
  requirements in the dashboard; remove test data and unrelated source files.
  Use screenshots of the actual extension without account identifiers or headers.
  The source package now includes Tide's existing 16/32/48/128 PNG icon resources,
  with manifest references and dimensions checked by the packager. Listing
  screenshots and promotional images still require review/preparation.
- [ ] Re-run controller tests and packaging validation, inspect the generated
  Chromium ZIP, record its commit and checksum, and review dependency notices.
- [ ] Create or obtain the intended store item/extension identity and match its
  actual ID in helper setup. An unpacked ID is not proof of the eventual store
  ID. Never permit wildcard native origins to avoid this step.
- [ ] Supply reviewer instructions and a publicly obtainable matching installer:
  Windows x64 prerequisite, helper registration, **Check local connection**,
  sign-in in Music, explicit **Import selected tab**, Library interaction and
  verification in Tide. Explain failure/retry and removal paths.
- [ ] Plan reviewer testing without sharing a maintainer's session cookies,
  Google password, private profile or Premium credentials. The publisher must
  resolve any access requirements through the review process; synthetic tests
  alone do not prove authenticated import works.
- [ ] Validate fresh install/update/uninstall with the real store-installed ID,
  account choice, cancellation, browser closure, Tide restart and session
  continuity. Repeat the real Chrome check for the submitted version. Windows
  integration CI does not prove authenticated browser import or GUI clicks.
- [ ] Describe known limits: unofficial service integration, no session-lifetime
  guarantee, no guaranteed Premium entitlement, unresolved Zen lookup, and no
  automatic extension installation. Keep support and security-report links usable.

## Human publisher actions

- [ ] The human owner chooses/authorizes the developer account, accepts terms,
  handles authentication/verification, pays any required registration fee, and
  reviews legal/policy and data-use certifications. Do not share login secrets
  with an assistant or commit payment/authentication data.
- [ ] The owner reviews the final listing, policy and artifact and explicitly
  authorizes submission. Store review may require changes or reject this approach;
  source availability and passing tests do not guarantee acceptance.
- [ ] After approval, verify the actual public listing, installed extension ID,
  published privacy link and native-helper connection before advertising store
  availability. Retain the documented unpacked beta fallback until appropriate.

Technical basis: [companion implementation and tests](../browser-companion/README.md),
[Chrome beta setup](BROWSER_COMPANION_BETA.md),
[account storage/playback details](ACCOUNT_SETUP.md), and
[browser acceptance evidence](BROWSER_CONNECTION.md#acceptance-evidence).
