"""Unit tests for the gateway's pure pieces (run: python3 -m unittest
discover gateway). The RNS/zeroconf-touching parts are exercised on real
hardware — these pin the relay codec and the proxy's local answers."""

import json
import unittest

import msgpack

from rns_gateway import (
    HEADER_ALLOWLIST,
    allocate_port,
    blobs_already_stored,
    cache_key,
    canned_health,
    pack_request,
    pack_response,
    reticulum_config,
)


class PackingTests(unittest.TestCase):
    def test_request_roundtrip_matches_design_shape(self):
        packed = pack_request(
            "POST",
            "/blips/get",
            {"Content-Type": "application/json", "User-Agent": "reqwest"},
            b'{"topics":{}}',
        )
        req = msgpack.unpackb(packed)
        self.assertEqual(req["m"], "POST")
        self.assertEqual(req["p"], "/blips/get")
        self.assertEqual(req["b"], b'{"topics":{}}')
        # Allowlist, never denylist: User-Agent must not ride the radio.
        self.assertEqual(list(req["h"].keys()), ["Content-Type"])

    def test_headers_absent_when_not_provided(self):
        req = msgpack.unpackb(pack_request("POST", "/p", {}, b""))
        self.assertEqual(req["h"], {})
        self.assertEqual(req["b"], b"")

    def test_response_roundtrip(self):
        r = msgpack.unpackb(pack_response(201, "application/json", b"{}"))
        self.assertEqual((r["s"], r["h"]["Content-Type"], r["b"]), (201, "application/json", b"{}"))

    def test_allowlist_is_minimal(self):
        # Growing this means more airtime per request — keep it deliberate.
        self.assertEqual(HEADER_ALLOWLIST, ("Content-Type",))


class CacheKeyTests(unittest.TestCase):
    def test_key_depends_on_body(self):
        a = cache_key("POST", "/blips/get", b'{"topics":{"t":{"a":0}}}')
        b = cache_key("POST", "/blips/get", b'{"topics":{"t":{"a":7}}}')
        self.assertNotEqual(a, b)

    def test_key_is_stable(self):
        self.assertEqual(
            cache_key("POST", "/p", b"x"),
            cache_key("POST", "/p", b"x"),
        )


class PortMapTests(unittest.TestCase):
    def test_ports_are_stable_and_disjoint(self):
        ports = {}
        a = allocate_port(ports, "aa", 8800)
        b = allocate_port(ports, "bb", 8800)
        self.assertEqual(a, 8800)
        self.assertEqual(b, 8801)
        self.assertEqual(allocate_port(ports, "aa", 8800), 8800)

    def test_persisted_ports_survive_and_gaps_fill(self):
        ports = {"old": 8801}
        self.assertEqual(allocate_port(ports, "new", 8800), 8800)
        self.assertEqual(allocate_port(ports, "newer", 8800), 8802)


class LocalAnswerTests(unittest.TestCase):
    def test_blob_announces_are_neutralized(self):
        body = json.dumps({"blob_hashes": ["h1", "h2"], "sender_pubkey": "x"}).encode()
        resp = json.loads(blobs_already_stored(body))
        self.assertEqual(resp, {"already_stored": ["h1", "h2"]})

    def test_blob_answer_tolerates_garbage(self):
        self.assertEqual(json.loads(blobs_already_stored(b"nope")), {"already_stored": []})

    def test_canned_health_has_manager_fields(self):
        h = json.loads(canned_health("MAILBOX-ID"))
        self.assertEqual(h["status"], "ok")
        self.assertEqual(h["endpoint_id"], "MAILBOX-ID")
        self.assertIn("endpoint_addr", h)


class ReticulumConfigTests(unittest.TestCase):
    def test_config_is_modem_only(self):
        class Args:
            rnode_port = "/dev/ttyACM0"
            rnode_freq = 869525000
            rnode_bandwidth = 125000
            rnode_sf = 8
            rnode_cr = 5
            rnode_txpower = 7

        cfg = reticulum_config(Args())
        self.assertIn("type = RNodeInterface", cfg)
        self.assertIn("port = /dev/ttyACM0", cfg)
        self.assertIn("frequency = 869525000", cfg)
        # The LAN must never become a mesh segment.
        self.assertIn("enable_transport = No", cfg)
        self.assertNotIn("AutoInterface", cfg)


if __name__ == "__main__":
    unittest.main()
