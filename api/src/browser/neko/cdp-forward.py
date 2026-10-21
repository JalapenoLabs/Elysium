# Copyright © 2026 Jalapeno Labs
"""Passes the browser's DevTools port to the container's network.

Headful Chromium binds --remote-debugging-port to loopback and ignores
--remote-debugging-address, so nothing outside the container could reach it. This listens on
every interface and relays each connection to it byte for byte. Only the API can connect: the
container's network is joined by the API and nginx alone, and nginx forwards only the media
port. Python is in the image already, because Neko's supervisord runs on it.
"""

import asyncio
import sys

LISTEN_PORT = 9223
CHROMIUM_PORT = 9222
CHUNK_BYTES = 65536


async def pipe(reader, writer):
    try:
        while data := await reader.read(CHUNK_BYTES):
            writer.write(data)
            await writer.drain()
    finally:
        writer.close()


async def forward(client_reader, client_writer):
    try:
        chromium_reader, chromium_writer = await asyncio.open_connection("127.0.0.1", CHROMIUM_PORT)
    except OSError as error:
        # Chromium is still starting, or restarting under supervisord.
        print(f"cdp-forward: Chromium refused a connection: {error}", file=sys.stderr, flush=True)
        client_writer.close()
        return
    await asyncio.gather(
        pipe(client_reader, chromium_writer),
        pipe(chromium_reader, client_writer),
        return_exceptions=True,
    )


async def main():
    server = await asyncio.start_server(forward, "0.0.0.0", LISTEN_PORT)
    async with server:
        await server.serve_forever()


asyncio.run(main())
