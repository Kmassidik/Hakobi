You are implementing Hakobi CP3.

Read `task.md` in this directory, then inspect the repository before editing.
CP2 already added a typed host-agent Docker protocol and a single env-configured
local provider. Your task is to add a catalog-backed multi-host registry so an
operator can add remote agents from the UI, each with encrypted credentials,
provider mapping, test/toggle/delete actions, and aggregated overview data.

Follow the task exactly. Keep all output in English. Do not deploy, push, commit,
modify the live host, or expose secrets. Preserve direct Docker providers and the
injected fake-backend path. Do not add a raw Docker proxy or arbitrary shell
endpoint.

When finished, report changed files, schema/API decisions, security decisions,
tests run, and remaining limitations.
