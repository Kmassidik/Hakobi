"""Process entry point: ``python3 -m build_runner``."""

from __future__ import annotations

import logging
import os
import signal
import sys
import threading

from .config import ConfigError, Settings, log_settings
from .server import create_server

logger = logging.getLogger("build_runner")


def main(argv: list[str] | None = None) -> int:
    del argv  # no command-line flags: configuration is environment-only

    level_name = os.environ.get("BUILD_RUNNER_LOG_LEVEL", "INFO").upper()
    level = getattr(logging, level_name, logging.INFO)
    if not isinstance(level, int):
        level = logging.INFO
    logging.basicConfig(
        level=level,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        stream=sys.stderr,
    )

    try:
        settings = Settings.from_env()
        settings.validate_runtime()
    except ConfigError as exc:
        logger.error("configuration error: %s", exc)
        return 2

    log_settings(settings)
    try:
        server = create_server(settings)
    except (ConfigError, OSError) as exc:
        logger.error("cannot start server: %s", exc)
        return 1

    def request_shutdown(signum: int, _frame: object) -> None:
        logger.info("received signal %d; shutting down", signum)
        # shutdown() blocks until serve_forever() returns, so it must run off
        # the thread that is currently serving.
        threading.Thread(target=server.shutdown, name="shutdown", daemon=True).start()

    signal.signal(signal.SIGTERM, request_shutdown)
    signal.signal(signal.SIGINT, request_shutdown)

    bound_host, bound_port = server.server_address[:2]
    logger.info("listening on http://%s:%d", bound_host, bound_port)
    try:
        server.serve_forever(poll_interval=0.5)
    except KeyboardInterrupt:  # pragma: no cover - signal handler usually wins
        pass
    finally:
        server.server_close()
    logger.info("stopped")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
