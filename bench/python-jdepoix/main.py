"""Bench harness for jdepoix/youtube-transcript-api against the local mock.

Monkey-patches the package's URL constants so it points at 127.0.0.1:18080.
Same modes as the Rust/TS clients (parse, parse-json, e2e, cold, memory) so
the run.py harness can compare apples-to-apples.
"""
import json
import sys
import time
from pathlib import Path

import youtube_transcript_api._settings as _settings
import youtube_transcript_api._transcripts as _transcripts

MOCK = "http://127.0.0.1:18080"
# Patch BOTH the source-of-truth and the imported names in _transcripts (it
# imports them directly: `from ._settings import WATCH_URL, ...`).
_settings.WATCH_URL = MOCK + "/watch?v={video_id}"
_settings.INNERTUBE_API_URL = MOCK + "/youtubei/v1/player?key={api_key}"
_transcripts.WATCH_URL = _settings.WATCH_URL
_transcripts.INNERTUBE_API_URL = _settings.INNERTUBE_API_URL

# The library also parses caption track URLs from the Innertube response, but
# it builds the timedtext request URL dynamically. Our canned innertube.json
# already contains absolute URLs pointing at our mock's /caption endpoint.

from youtube_transcript_api import YouTubeTranscriptApi  # noqa: E402

VIDEO_ID = "dQw4w9WgXcQ"


def read_self_vmhwm_kib():
    try:
        with open("/proc/self/status") as f:
            for line in f:
                if line.startswith("VmHWM:"):
                    return int(line.split()[1])
    except Exception:
        pass
    return 0


def fetch_once():
    api = YouTubeTranscriptApi()
    transcript = api.fetch(VIDEO_ID, languages=("en",))
    # The fetch() result is a FetchedTranscript; convert to plain text.
    return " ".join(s.text for s in transcript)


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "cold"
    iters = int(sys.argv[2]) if len(sys.argv) > 2 else 100

    if mode == "parse":
        # Use the library's own internal parser on the canned XML.
        from youtube_transcript_api._transcripts import _TranscriptParser
        xml = Path("/tmp/bench/canned/caption.xml").read_text()
        parser = _TranscriptParser()
        # Library API: parse(xml) → list[FetchedTranscriptSnippet]
        t0 = time.perf_counter_ns()
        bytes_out = 0
        for _ in range(iters):
            snippets = parser.parse(xml)
            for s in snippets:
                bytes_out += len(s.text)
        ms = (time.perf_counter_ns() - t0) / 1e6
        print(json.dumps({"mode": "parse", "iters": iters, "ms": ms, "bytesIn": len(xml), "bytesOut": bytes_out}))

    elif mode == "parse-json":
        txt = Path("/tmp/bench/canned/innertube.json").read_text()
        t0 = time.perf_counter_ns()
        n = 0
        for _ in range(iters):
            j = json.loads(txt)
            n += len(j.get("captions", {}).get("playerCaptionsTracklistRenderer", {}).get("captionTracks", []))
        ms = (time.perf_counter_ns() - t0) / 1e6
        print(json.dumps({"mode": "parse-json", "iters": iters, "ms": ms, "n": n}))

    elif mode == "e2e":
        samples = []
        for i in range(iters):
            t0 = time.perf_counter_ns()
            r = fetch_once()
            samples.append((time.perf_counter_ns() - t0) / 1e6)
            if i == 0 and not r:
                raise SystemExit("empty result")
        print(json.dumps({"mode": "e2e", "iters": iters, "samples": samples}))

    elif mode == "cold":
        t0 = time.perf_counter_ns()
        r = fetch_once()
        ms = (time.perf_counter_ns() - t0) / 1e6
        print(json.dumps({"mode": "cold", "ms": ms, "len": len(r)}))

    elif mode == "memory":
        for _ in range(iters):
            fetch_once()
        time.sleep(0.25)
        print(json.dumps({"mode": "memory", "iters": iters, "rssKib": read_self_vmhwm_kib()}))

    else:
        raise SystemExit(f"unknown mode: {mode}")


if __name__ == "__main__":
    main()
