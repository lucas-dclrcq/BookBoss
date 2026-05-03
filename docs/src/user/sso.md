# Single Sign-On (OIDC)

BookBoss can authenticate users against an external **OpenID Connect (OIDC)** identity provider
(Kanidm, Keycloak, Authentik, Authelia, etc.) in addition to the built-in username/password
login. Username/password login remains available even when SSO is enabled — SSO is additive.

## Enabling SSO

Set the following environment variables. SSO becomes available once all three of
`DISCOVERY_URL`, `CLIENT_ID`, and `CLIENT_SECRET` are present:

| Variable                        | Description                                          | Default            |
| ------------------------------- | ---------------------------------------------------- | ------------------ |
| `BOOKBOSS__OIDC__DISCOVERY_URL` | OIDC discovery URL (e.g. `https://idp.example.com`)  | —                  |
| `BOOKBOSS__OIDC__CLIENT_ID`     | OIDC client ID registered with the IdP               | —                  |
| `BOOKBOSS__OIDC__CLIENT_SECRET` | OIDC client secret registered with the IdP           | —                  |
| `BOOKBOSS__OIDC__BUTTON_LABEL`  | Label shown on the SSO button on the login page      | `Sign in with SSO` |

When SSO is enabled, a **Sign in with SSO** button appears on the login page next to the
username/password form. If the OIDC provider is unreachable, the button soft-fails and login
falls back to username/password.

## How User Matching Works

BookBoss matches the IdP user to a local BookBoss account by **email address**:

1. The user clicks **Sign in with SSO** and authenticates with the IdP.
2. The IdP returns an ID token containing the `email` claim.
3. BookBoss looks up a user account with that exact email address (case-sensitive match).
4. If a match is found, the user is logged in. If not, the login fails.

> BookBoss does not auto-provision users. The matching account must already exist — create it
> from **Settings → Users** before the user attempts SSO login for the first time.

## Registering BookBoss with Your IdP

When configuring BookBoss as an OAuth2/OIDC client in your IdP, use:

- **Redirect URI:** `<base_url>/auth/oidc/callback`
- **Scopes:** `openid email`
- **Grant type:** `authorization_code` with **PKCE** enabled

`<base_url>` must match your `BOOKBOSS__FRONTEND__BASE_URL` (e.g. `https://bookboss.example.com`).
The IdP must release the `email` claim — without it, BookBoss cannot match the user to a local
account.

## Example: Kanidm

The following is a working setup for [Kanidm](https://kanidm.com/):

```bash
# Create a group for BookBoss users and add members
kanidm group create 'bookboss_users'
kanidm group add-members bookboss_users <user>

# Register BookBoss as an OAuth2 client
kanidm system oauth2 create bookboss '<machine>' '<base_url>'
kanidm system oauth2 set-landing-url bookboss '<base_url>/auth/oidc/callback'
kanidm system oauth2 update-scope-map bookboss bookboss_users email openid
kanidm system oauth2 enable-pkce bookboss

# Reveal the client secret — copy this into BOOKBOSS__OIDC__CLIENT_SECRET
kanidm system oauth2 show-basic-secret bookboss
```

The matching BookBoss environment variables for this Kanidm setup are:

```bash
BOOKBOSS__OIDC__DISCOVERY_URL=https://idm.example.com/oauth2/openid/bookboss/.well-known/openid-configuration
BOOKBOSS__OIDC__CLIENT_ID=bookboss
BOOKBOSS__OIDC__CLIENT_SECRET=<output of `kanidm system oauth2 show-basic-secret bookboss`>
# Optional — override the default "Sign in with SSO" button label
BOOKBOSS__OIDC__BUTTON_LABEL=Sign in with Kanidm
```

The discovery URL for Kanidm follows the pattern
`https://<kanidm-host>/oauth2/openid/<client-id>/.well-known/openid-configuration` — replace
`<kanidm-host>` with your Kanidm domain and `<client-id>` with the name you used in
`kanidm system oauth2 create` (`bookboss` in the example above).

After setting these and restarting BookBoss, the **Sign in with SSO** button will appear on
the login page.

## Troubleshooting

**SSO button does not appear**

- Confirm all three of `DISCOVERY_URL`, `CLIENT_ID`, and `CLIENT_SECRET` are set in the BookBoss
  environment. The button only renders when all three are present.
- Check the BookBoss logs at startup — discovery failures are logged with the full error chain.

**SSO login fails with "login failed"**

- The most common cause is no BookBoss user with a matching email address. Create the user
  account first from **Settings → Users**, using the email address the IdP releases.
- Email matching is case-sensitive. If the IdP releases `Alice@Example.COM` and the BookBoss
  user has `alice@example.com`, the match will fail.
- Confirm the IdP is releasing the `email` claim in the ID token (some IdPs require this to be
  explicitly mapped to the client).

**Discovery or token exchange errors**

- BookBoss surfaces the full error chain on the login page when SSO callback fails. The logs
  include INFO breadcrumbs for each step of the OIDC flow — discovery, token exchange, claim
  validation — to help pinpoint where the failure occurred.
