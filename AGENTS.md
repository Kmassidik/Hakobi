# Hakobi execution protocol

Follow this order unless the user explicitly overrides it.

1. **Map first**
   - Identify host, access path, target domain, runtime, and data location.
   - Verify credentials and reachability before changing code or installing tools.
   - Do not jump to a different host because one path is blocked.

2. **Audit before editing**
   - Capture every affected page at desktop and mobile sizes.
   - Note concrete defects: overflow, clipping, contrast, stale assets, missing
     icons, CSP failures, broken empty states.
   - Do not fix one page in isolation when the same defect is repeated.

3. **Iterate UI locally**
   - Use `cargo run` against a local data directory for CSS/template work.
   - Do not rebuild Docker for every visual change. Assets are compiled into the
     binary via `RustEmbed`, so a rebuild is only required when verifying the
     deployable image.
   - Rebuild the CSS with `npm run build:css` after template/class changes.

4. **Verify the real viewport**
   - Screenshot desktop at 1440 px.
   - Use CDP `Emulation.setDeviceMetricsOverride` for 390 px mobile; Chromium's
     `--window-size=390` can enforce a 500 px minimum layout viewport and produce
     false clipping.
   - Test interactive controls, including password visibility, forms, and
     destructive confirmation flows.

5. **Build and deploy once**
   - After visual verification passes, build the image once.
   - Use the cargo-chef Docker stages so dependency compilation is cached and a
     static/template change only rebuilds the Hakobi crate.
   - Deploy, run a smoke check, then commit and push with the branch and commit
     message describing what, where, why, verification, and risk.

## Git and local memory

Every meaningful change is recorded in both:

- the Git branch name and commit message,
- the local working tree and `docs/WORKING_AGREEMENT.md`.

Never commit secrets, environment files, build output, or agent run artifacts.
