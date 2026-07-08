#!/usr/bin/env python3
"""Dash Chat RNS mailbox gateway (see docs/rns-gateway.md).

Syncs Dash Chat mailboxes over a LoRa link with no IP connectivity between
them, changing nothing in the mailbox: Reticulum (RNS) moves opaque bytes
between gateways, each gateway relays HTTP at the application layer against
its *local* mailbox, and every discovered LoRa peer is advertised on the LAN
as its own mDNS service on its own port — so the mailbox manager discovers a
LoRa peer through the exact `_dashchat._tcp.local.` path it already uses.

The mailbox's HTTP client has a hard 10 s timeout, far below LoRa RTTs, so
the per-peer proxy never blocks on the radio:

- ``POST /blips/get``   → answered from a request-keyed cache. Miss: 503 now,
  radio exchange in the background; the manager's next 30 s poll (same
  heights → same key) gets the real response inline.
- ``POST /blips/store`` → 201 immediately, relayed in the background with
  retries (blip inserts are idempotent; the far side keeps listing what it
  lacks, so a dropped publish is re-driven by the manager itself).
- ``POST /blobs/store`` → answered locally with "already stored": blips
  only, no blob bytes over the radio.
- ``/health``, ``/peers/register`` → answered locally (never relayed; the
  manager doesn't health-check LAN peers).

The gateway owns all RNS state (persisted identity, announce cadence, warm
links, per-peer ports); the mailbox owns persistence and trust.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import queue
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import msgpack

APP_NAME = "dashchat"
ASPECT = "mailbox"
SERVICE_TYPE = "_dashchat._tcp.local."

# Only what the far mailbox needs to process the request; everything else is
# airtime-metered envelope (the far HTTP client regenerates Host and
# Content-Length itself).
HEADER_ALLOWLIST = ("Content-Type",)

GET_CACHE_TTL = 300.0  # seconds a relayed response answers repeat requests
GET_CACHE_MAX = 64  # per peer
STORE_RETRIES = 8
LOCAL_HTTP_TIMEOUT = 30  # loopback mailbox calls
LINK_ESTABLISH_TIMEOUT = 120  # several LoRa round trips
REQUEST_TIMEOUT = 900  # a multi-KB Resource transfer can take minutes


# --------------------------------------------------------------------------
# Pure helpers (unit-tested in test_gateway.py)


def pack_request(method: str, path: str, headers, body: bytes) -> bytes:
    """Serialize an HTTP request semantically — allowlisted headers only,
    never raw wire bytes."""
    return msgpack.packb(
        {
            "m": method,
            "p": path,
            "h": {k: headers[k] for k in HEADER_ALLOWLIST if headers.get(k)},
            "b": body or b"",
        }
    )


def pack_response(status: int, content_type: str, body: bytes) -> bytes:
    return msgpack.packb({"s": status, "h": {"Content-Type": content_type}, "b": body})


def cache_key(method: str, path: str, body: bytes) -> str:
    h = hashlib.sha256()
    h.update(method.encode())
    h.update(b"\0")
    h.update(path.encode())
    h.update(b"\0")
    h.update(body or b"")
    return h.hexdigest()


def allocate_port(ports: dict[str, int], dest_hash: str, base: int) -> int:
    """A stable port per peer, persisted across restarts so the mDNS record
    never churns; new peers get the lowest free port from `base`."""
    if dest_hash in ports:
        return ports[dest_hash]
    used = set(ports.values())
    port = base
    while port in used:
        port += 1
    ports[dest_hash] = port
    return port


def blobs_already_stored(body: bytes) -> bytes:
    """The canned /blobs/store answer: every announced hash is 'already
    stored', so the peer never pushes blob bytes at the radio."""
    try:
        hashes = json.loads(body or b"{}").get("blob_hashes", [])
    except (ValueError, AttributeError):
        hashes = []
    return json.dumps({"already_stored": hashes}).encode()


def canned_health(mailbox_id: str) -> bytes:
    # Never relayed: LAN peers aren't health-checked by the manager (this
    # answer exists for humans with curl). endpoint_addr is a placeholder —
    # there is no iroh dialing across the radio anyway.
    return json.dumps(
        {
            "status": "ok",
            "endpoint_id": mailbox_id,
            "endpoint_addr": {"id": "", "addrs": []},
        }
    ).encode()


def local_ipv4s() -> list[str]:
    """The host's IPv4 addresses for the mDNS A records. Loopback is always
    included — the mailbox's discovery TCP-probes routable addresses first
    and falls back to loopback, which is exactly the same-host case."""
    addrs = []
    try:
        out = subprocess.run(
            ["ip", "-j", "-4", "addr", "show"],
            capture_output=True,
            timeout=5,
            check=True,
        ).stdout
        for iface in json.loads(out):
            for a in iface.get("addr_info", []):
                ip = a.get("local")
                if ip and ip != "127.0.0.1":
                    addrs.append(ip)
    except Exception:  # noqa: BLE001 — no `ip`? loopback still works
        pass
    addrs.append("127.0.0.1")
    return addrs


def reticulum_config(args) -> str:
    """The generated Reticulum config: transport off, no AutoInterface (the
    LAN must not become a mesh segment), just the RNode as a modem."""
    return f"""# Generated by rns-gateway from lora.env — do not edit.
[reticulum]
  enable_transport = No
  share_instance = No
  panic_on_interface_error = No

[logging]
  loglevel = 3

[interfaces]
  [[RNode LoRa]]
    type = RNodeInterface
    enabled = Yes
    port = {args.rnode_port}
    frequency = {args.rnode_freq}
    bandwidth = {args.rnode_bandwidth}
    txpower = {args.rnode_txpower}
    spreadingfactor = {args.rnode_sf}
    codingrate = {args.rnode_cr}
"""


# --------------------------------------------------------------------------
# Gateway


class Gateway:
    def __init__(self, args):
        import RNS
        from zeroconf import Zeroconf

        self.args = args
        self.RNS = RNS
        self.zeroconf = Zeroconf()
        self.peers: dict[bytes, Peer] = {}
        self.peers_lock = threading.Lock()
        self.jobs: queue.Queue = queue.Queue()  # one radio → one global queue
        self.ports_path = os.path.join(args.data_dir, "ports.json")
        self.ports: dict[str, int] = {}
        self.local_mailbox_id = None
        self.last_announce_sent = 0.0

    # -- startup ------------------------------------------------------------

    def start(self):
        RNS = self.RNS
        os.makedirs(self.args.data_dir, exist_ok=True)

        if os.path.exists(self.ports_path):
            with open(self.ports_path) as f:
                self.ports = json.load(f)

        # Reticulum, with our generated config (the RNode is the only interface).
        configdir = os.path.join(self.args.data_dir, "reticulum")
        os.makedirs(configdir, exist_ok=True)
        with open(os.path.join(configdir, "config"), "w") as f:
            f.write(reticulum_config(self.args))
        RNS.Reticulum(configdir=configdir)

        # Persisted identity: the dest_hash IS our mesh identity — if it
        # churned on restart, every peer would re-register us as brand new.
        identity_path = os.path.join(self.args.data_dir, "rns_identity")
        if os.path.exists(identity_path):
            self.identity = RNS.Identity.from_file(identity_path)
        else:
            self.identity = RNS.Identity()
            self.identity.to_file(identity_path)

        self.in_dest = RNS.Destination(
            self.identity,
            RNS.Destination.IN,
            RNS.Destination.SINGLE,
            APP_NAME,
            ASPECT,
        )
        self.in_dest.set_proof_strategy(RNS.Destination.PROVE_ALL)
        self.in_dest.register_request_handler(
            "/http",
            response_generator=self.serve_relayed_request,
            allow=RNS.Destination.ALLOW_ALL,  # auth is the mailbox's job
        )
        RNS.log(f"[gateway] destination {RNS.prettyhexrep(self.in_dest.hash)}")

        # Our announce carries the local MailboxId so the far gateway can
        # advertise us under the name the far manager expects.
        self.local_mailbox_id = self.wait_local_mailbox_id()

        gateway = self

        class Handler:
            aspect_filter = f"{APP_NAME}.{ASPECT}"
            receive_path_responses = True

            def received_announce(self, destination_hash, announced_identity, app_data):
                gateway.on_announce(destination_hash, announced_identity, app_data)

        RNS.Transport.register_announce_handler(Handler())

        threading.Thread(target=self.announce_loop, daemon=True).start()
        threading.Thread(target=self.relay_worker, daemon=True).start()
        threading.Thread(target=self.reaper_loop, daemon=True).start()

    def wait_local_mailbox_id(self) -> str:
        url = f"{self.args.mailbox_url}/health"
        while True:
            try:
                with urllib.request.urlopen(url, timeout=5) as resp:
                    mailbox_id = json.load(resp)["endpoint_id"]
                    self.RNS.log(f"[gateway] local mailbox: {mailbox_id}")
                    return mailbox_id
            except Exception as e:  # noqa: BLE001
                self.RNS.log(f"[gateway] waiting for local mailbox: {e}")
                time.sleep(5)

    # -- serving side: answer peers over the radio ---------------------------

    def serve_relayed_request(
        self, path, data, request_id, link_id, remote_identity, requested_at
    ):
        """Unpack a relayed HTTP request and replay it as a fresh local HTTP
        call against our mailbox; TCP terminates here at both ends."""
        req = msgpack.unpackb(data)
        url = f"{self.args.mailbox_url}{req['p']}"
        r = urllib.request.Request(
            url, data=req["b"] or None, method=req["m"], headers=req["h"]
        )
        try:
            with urllib.request.urlopen(r, timeout=LOCAL_HTTP_TIMEOUT) as resp:
                status, ctype, body = (
                    resp.status,
                    resp.headers.get("Content-Type", ""),
                    resp.read(),
                )
        except urllib.error.HTTPError as e:
            status, ctype, body = e.code, e.headers.get("Content-Type", ""), e.read()
        except Exception as e:  # noqa: BLE001 — mailbox briefly down
            self.RNS.log(f"[gateway] local replay failed: {e}")
            status, ctype, body = 502, "text/plain", b"local mailbox unreachable"
        self.RNS.log(f"[gateway] relayed {req['m']} {req['p']} -> {status}")
        return pack_response(status, ctype, body)

    # -- discovery side: LoRa peers become LAN mailboxes ---------------------

    def on_announce(self, dest_hash, announced_identity, app_data):
        if not app_data:
            return
        mailbox_id = app_data.decode(errors="replace")
        if mailbox_id == self.local_mailbox_id:
            return
        with self.peers_lock:
            peer = self.peers.get(dest_hash)
            if peer is None:
                port = allocate_port(
                    self.ports, self.RNS.hexrep(dest_hash, delimit=False), self.args.port_base
                )
                self.save_ports()
                peer = Peer(self, dest_hash, announced_identity, mailbox_id, port)
                self.peers[dest_hash] = peer
                peer.start()
                self.RNS.log(
                    f"[gateway] peer {mailbox_id} up on :{port} "
                    f"({self.RNS.prettyhexrep(dest_hash)})"
                )
                # Help mutual discovery: they may not have heard us yet.
                if time.time() - self.last_announce_sent > 60:
                    self.send_announce()
            peer.last_announce = time.time()

    def save_ports(self):
        with open(self.ports_path, "w") as f:
            json.dump(self.ports, f)

    def send_announce(self):
        self.in_dest.announce(app_data=self.local_mailbox_id.encode())
        self.last_announce_sent = time.time()

    def announce_loop(self):
        # Announces cost airtime: tiny app_data, minutes-scale interval.
        while True:
            self.send_announce()
            time.sleep(self.args.announce_interval)

    def reaper_loop(self):
        # Withdraw peers only after a real timeout, not one missed announce:
        # LoRa announces are lossy, and a flapping mDNS record makes the
        # manager chase ghosts. Ports stay reserved.
        while True:
            time.sleep(60)
            cutoff = time.time() - self.args.peer_timeout
            with self.peers_lock:
                stale = [p for p in self.peers.values() if p.last_announce < cutoff]
                for peer in stale:
                    self.RNS.log(f"[gateway] reaping silent peer {peer.mailbox_id}")
                    del self.peers[peer.dest_hash]
                    peer.stop()

    # -- the radio worker -----------------------------------------------------

    def relay_worker(self):
        while True:
            job = self.jobs.get()
            if job.not_before > time.time():
                self.jobs.put(job)
                time.sleep(0.5)
                continue
            with self.peers_lock:
                alive = job.peer.dest_hash in self.peers
            if not alive:
                continue
            try:
                response = job.peer.exchange(job.packed)
            except Exception as e:  # noqa: BLE001
                self.RNS.log(f"[gateway] exchange with {job.peer.mailbox_id} failed: {e}")
                job.peer.pending_gets.discard(job.key)
                if job.kind == "store":
                    job.attempts += 1
                    if job.attempts < STORE_RETRIES:
                        job.not_before = time.time() + 30 * job.attempts
                        self.jobs.put(job)
                    else:
                        self.RNS.log(
                            "[gateway] dropping store after retries "
                            "(the manager re-drives missing blips)"
                        )
                continue
            if job.kind == "get":
                job.peer.cache_put(job.key, response)
                job.peer.pending_gets.discard(job.key)


class RelayJob:
    def __init__(self, kind: str, peer: "Peer", packed: bytes, key: str = ""):
        self.kind = kind
        self.peer = peer
        self.packed = packed
        self.key = key
        self.attempts = 0
        self.not_before = 0.0


class Peer:
    """One LoRa peer: its warm link, its response cache, its LAN front — a
    dedicated HTTP listener plus an mDNS record under the peer's MailboxId.
    The port itself is the routing: this handler closes over the peer."""

    def __init__(self, gateway: Gateway, dest_hash, identity, mailbox_id: str, port: int):
        self.gateway = gateway
        self.dest_hash = dest_hash
        self.identity = identity
        self.mailbox_id = mailbox_id
        self.port = port
        self.last_announce = time.time()
        self.link = None
        self.link_lock = threading.Lock()
        self.cache: dict[str, tuple[float, bytes]] = {}
        self.cache_lock = threading.Lock()
        self.pending_gets: set[str] = set()
        self.server = None
        self.service_info = None

    # -- LAN front ------------------------------------------------------------

    def start(self):
        from zeroconf import ServiceInfo

        peer = self

        class ProxyHandler(BaseHTTPRequestHandler):
            def log_message(self, *a):  # RNS.log instead of stderr
                pass

            def _read_body(self) -> bytes:
                n = int(self.headers.get("Content-Length", 0) or 0)
                return self.rfile.read(n) if n else b""

            def _reply(self, status: int, body: bytes, ctype="application/json"):
                self.send_response(status)
                if body:
                    self.send_header("Content-Type", ctype)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_POST(self):
                body = self._read_body()
                if self.path == "/blips/store":
                    # Store-and-forward: accept now, relay in the background.
                    peer.enqueue_store(self.command, self.path, self.headers, body)
                    self._reply(201, b"")
                elif self.path == "/blips/get":
                    hit = peer.serve_cached_or_schedule(self.command, self.path, self.headers, body)
                    if hit is not None:
                        self._reply(200, hit)
                    else:
                        self._reply(503, b"radio exchange in flight; retry next cycle", "text/plain")
                elif self.path == "/blobs/store":
                    self._reply(200, blobs_already_stored(body))
                elif self.path == "/peers/register":
                    self._reply(204, b"")
                else:
                    self._reply(404, b"")

            def do_GET(self):
                if self.path == "/health":
                    self._reply(200, canned_health(peer.mailbox_id))
                else:
                    self._reply(404, b"")

        self.server = ThreadingHTTPServer((self.gateway.args.bind, self.port), ProxyHandler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

        self.service_info = ServiceInfo(
            SERVICE_TYPE,
            f"{self.mailbox_id}.{SERVICE_TYPE}",
            addresses=[socket.inet_aton(ip) for ip in local_ipv4s()],
            port=self.port,
            properties={},
            server=f"{socket.gethostname()}-gw{self.port}.local.",
        )
        self.gateway.zeroconf.register_service(self.service_info)

    def stop(self):
        if self.service_info is not None:
            # ServiceRemoved makes the manager unregister the peer.
            try:
                self.gateway.zeroconf.unregister_service(self.service_info)
            except Exception:  # noqa: BLE001
                pass
        if self.server is not None:
            self.server.shutdown()
        with self.link_lock:
            if self.link is not None:
                self.link.teardown()
                self.link = None

    # -- proxy behaviors --------------------------------------------------------

    def enqueue_store(self, method, path, headers, body):
        packed = pack_request(method, path, headers, body)
        self.gateway.jobs.put(RelayJob("store", self, packed))

    def serve_cached_or_schedule(self, method, path, headers, body):
        key = cache_key(method, path, body)
        with self.cache_lock:
            entry = self.cache.get(key)
            if entry and entry[0] > time.time():
                return entry[1]
        if key not in self.pending_gets:
            self.pending_gets.add(key)
            packed = pack_request(method, path, headers, body)
            self.gateway.jobs.put(RelayJob("get", self, packed, key))
        return None

    def cache_put(self, key: str, response: bytes):
        with self.cache_lock:
            self.cache[key] = (time.time() + GET_CACHE_TTL, response)
            while len(self.cache) > GET_CACHE_MAX:
                self.cache.pop(min(self.cache, key=lambda k: self.cache[k][0]))

    # -- the radio round trip ----------------------------------------------------

    def ensure_link(self):
        """One warm RNS.Link per peer, reused across sync cycles — link
        establishment is several LoRa round trips, never pay it per request."""
        RNS = self.gateway.RNS
        with self.link_lock:
            if self.link is not None and self.link.status == RNS.Link.ACTIVE:
                return self.link
            if not RNS.Transport.has_path(self.dest_hash):
                RNS.Transport.request_path(self.dest_hash)
                deadline = time.time() + LINK_ESTABLISH_TIMEOUT
                while not RNS.Transport.has_path(self.dest_hash):
                    if time.time() > deadline:
                        raise TimeoutError("no path to peer")
                    time.sleep(1)
            out_dest = RNS.Destination(
                self.identity,
                RNS.Destination.OUT,
                RNS.Destination.SINGLE,
                APP_NAME,
                ASPECT,
            )
            established = threading.Event()

            def on_closed(_link):
                with self.link_lock:
                    self.link = None

            link = RNS.Link(
                out_dest,
                established_callback=lambda _l: established.set(),
                closed_callback=on_closed,
            )
            if not established.wait(LINK_ESTABLISH_TIMEOUT):
                link.teardown()
                raise TimeoutError("link establishment timed out")
            self.link = link
            return link

    def exchange(self, packed: bytes) -> bytes:
        """One serialized-HTTP round trip over the link. Responses larger
        than a packet auto-upgrade to an RNS Resource (fragmentation,
        sequencing and checksumming included)."""
        link = self.ensure_link()
        done = threading.Event()
        result: dict = {}

        def on_response(receipt):
            result["response"] = receipt.response
            done.set()

        def on_failed(_receipt):
            done.set()

        receipt = link.request(
            "/http",
            data=packed,
            response_callback=on_response,
            failed_callback=on_failed,
            timeout=REQUEST_TIMEOUT,
        )
        if receipt is False or receipt is None:
            raise ConnectionError("request refused (link not ready)")
        if not done.wait(REQUEST_TIMEOUT + 30):
            raise TimeoutError("radio round trip timed out")
        if "response" not in result:
            raise ConnectionError("radio round trip failed")
        r = msgpack.unpackb(result["response"])
        if r["s"] >= 400:
            raise ConnectionError(f"far mailbox answered {r['s']}")
        return r["b"]


# --------------------------------------------------------------------------


def parse_args(argv=None):
    e = os.environ.get
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--mailbox-url", default=e("GATEWAY_MAILBOX_URL", "http://127.0.0.1:3000"))
    p.add_argument("--data-dir", default=e("GATEWAY_DATA_DIR", "/var/lib/rns-gateway"))
    p.add_argument("--bind", default=e("GATEWAY_BIND", "0.0.0.0"))
    p.add_argument("--port-base", type=int, default=int(e("GATEWAY_PORT_BASE", "8800")))
    p.add_argument(
        "--announce-interval", type=int, default=int(e("GATEWAY_ANNOUNCE_INTERVAL", "300"))
    )
    p.add_argument("--peer-timeout", type=int, default=int(e("GATEWAY_PEER_TIMEOUT", "900")))
    p.add_argument("--rnode-port", default=e("RNODE_PORT", "/dev/ttyACM0"))
    p.add_argument("--rnode-freq", type=int, default=int(e("RNODE_FREQ", "869525000")))
    p.add_argument("--rnode-bandwidth", type=int, default=int(e("RNODE_BANDWIDTH", "125000")))
    p.add_argument("--rnode-sf", type=int, default=int(e("RNODE_SF", "8")))
    p.add_argument("--rnode-cr", type=int, default=int(e("RNODE_CR", "5")))
    p.add_argument("--rnode-txpower", type=int, default=int(e("RNODE_TXPOWER", "7")))
    return p.parse_args(argv)


def main():
    gateway = Gateway(parse_args())
    gateway.start()
    signal.sigwait({signal.SIGINT, signal.SIGTERM})
    return 0


if __name__ == "__main__":
    sys.exit(main())
