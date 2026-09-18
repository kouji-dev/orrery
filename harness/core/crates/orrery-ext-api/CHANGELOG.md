# Changelog

## 0.2.0 — unreleased

**Breaking.** This crate is published, so the move below is a major bump under
pre-1.0 semver rather than a quiet addition.

### Added

- `creds`: the `creds` grant as a store, shared by every provider extension.
  `CredStore` (async: `get`, `put`, `clear`, `has`), `BrokerCredStore`,
  `EnvCredStore`, `MemoryCredStore` and `LayeredCredStore`.

  It is **moved**, not new: `orrery-ext-provider-anthropic` and
  `orrery-ext-provider-openai-compat` each had a copy, which is one copy too
  many for a thing whose whole job is to be the single place a secret lives. A
  third-party provider now gets the same type, and tests it against the same
  `testing::MockBroker`.

  The trait is **async**, where the copies were sync: a broker call is async, and
  a sync store could only ever have been backed by a file this crate picked.
  Callers move the read into the task that needs it — `Provider::stream` stays a
  plain fn, which is what lets the kernel hold `Arc<dyn Provider>`.

- `BrokerFacade::store_credential`, `forget_credential` and `has_credential`,
  each with a default `Unsupported` body. A new method with a default body is
  not a breaking change under `orrery-ext/1` (plan 06, open question 3); they
  are listed here because they are what a login flow persists a token with.
  Without them an extension would have to pick its own file, outside the `creds`
  grant and outside the ledger.

  `has_credential` answers "am I signed in" without reading a secret, which is
  what keeps `AuthState` honest on a keychain that hands nothing back.

- `testing::BrokerCall::StoreCredential` and `ForgetCredential`, so an
  extension's test can assert that a login wrote through the grant.
  `has_credential` is deliberately **not** recorded: `state()` asks it every
  turn and a ledger full of it would bury the reads that matter.

### Migration

```diff
-use orrery_ext_provider_anthropic::auth::{CredStore, MemoryCredStore};
+use orrery_ext_api::creds::{CredStore, MemoryCredStore};

-store.put("anthropic", key)?;
+store.put("anthropic", key).await?;
```

The error type changed with it: `ProviderError` became `BrokerError`, because
the store is a broker call and a denial should carry the rule that refused it.
`orrery_ext_provider_anthropic::auth::store_error` converts at the provider
boundary.
