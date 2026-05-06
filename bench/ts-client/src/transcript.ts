// Apples-to-apples TS port of the Rust core. Same algorithm, same wire shape.
// Uses fast-xml-parser (the standard Node XML parser) so the comparison is
// against a tuned, idiomatic TS implementation rather than something hand-rolled.

import { XMLParser } from "fast-xml-parser";

const VALID_HOSTS = new Set([
  "youtube.com", "www.youtube.com", "m.youtube.com", "youtu.be",
  "youtube.co.uk", "youtube.de", "youtube.fr", "youtube.jp",
  "youtube.ca", "youtube.es", "youtube.com.br", "youtube.co.in", "youtube.co.kr",
]);

const AUTO_DETECT_ORDER = ["en", "es", "fr", "de", "tr", "pt", "ja", "ko", "zh", "it", "ru", "ar"];

export type CaptionTrack = { baseUrl: string; languageCode: string };

export function extractVideoId(raw: string): string {
  const normalized = raw.startsWith("http") ? raw : `https://${raw}`;
  const u = new URL(normalized);
  const host = u.host.toLowerCase();
  const bare = host.startsWith("www.") ? host.slice(4) : host;
  if (!VALID_HOSTS.has(host) && !VALID_HOSTS.has(bare)) {
    throw new Error("Invalid YouTube URL");
  }
  let id: string | null = null;
  if (host === "youtu.be" || bare === "youtu.be") {
    id = u.pathname.split("/").filter(Boolean)[0] ?? null;
  } else if (u.pathname.startsWith("/watch")) {
    id = u.searchParams.get("v");
  } else if (
    u.pathname.startsWith("/shorts/") ||
    u.pathname.startsWith("/live/") ||
    u.pathname.startsWith("/embed/")
  ) {
    id = u.pathname.split("/")[2] ?? null;
  }
  if (!id || id.length !== 11 || !/^[A-Za-z0-9_-]{11}$/.test(id)) {
    throw new Error("Invalid video ID");
  }
  return id;
}

export function innertubeBody(videoId: string) {
  return {
    context: {
      client: {
        clientName: "ANDROID",
        clientVersion: "20.10.38",
        androidSdkVersion: 30,
        hl: "en",
        gl: "US",
      },
    },
    videoId,
  };
}

export function selectTrack(
  tracks: CaptionTrack[],
  language: string,
): { track: CaptionTrack; fallbackNote: string | null } {
  if (tracks.length === 0) throw new Error("No transcripts are available for this video");
  const lang = (language || "auto").toLowerCase();

  if (lang === "auto" || lang === "") {
    for (const code of AUTO_DETECT_ORDER) {
      const t = tracks.find((t) => t.languageCode === code);
      if (t) return { track: t, fallbackNote: null };
    }
    return { track: tracks[0], fallbackNote: null };
  }

  const exact = tracks.find((t) => t.languageCode === lang);
  if (exact) return { track: exact, fallbackNote: null };

  if (lang !== "en") {
    const en = tracks.find((t) => t.languageCode === "en");
    if (en) {
      return {
        track: en,
        fallbackNote: `Requested language '${lang}' not available, showing English instead`,
      };
    }
  }
  throw new Error(
    `Language '${lang}' not available; available languages: ${tracks.map((t) => t.languageCode).join(", ")}`,
  );
}

const PARSER = new XMLParser({ ignoreAttributes: true });

export function parseTranscriptXml(xml: string): string {
  const doc = PARSER.parse(xml);
  // Innertube format: <timedtext><body><p>...</p></body></timedtext>
  // Legacy format:    <transcript><text>...</text></transcript>
  const root = doc.transcript ?? doc.timedtext?.body ?? null;
  if (!root) return "";
  const elems = root.text ?? root.p ?? [];
  const arr = Array.isArray(elems) ? elems : [elems];
  return arr.map((e) => (typeof e === "string" ? e : (e["#text"] ?? "")).trim())
    .filter(Boolean)
    .join(" ");
}

export interface FetchOpts {
  innertubeUrl: string; // e.g. http://127.0.0.1:18080/youtubei/v1/player
}

export async function getTranscript(
  url: string,
  language: string,
  opts: FetchOpts,
): Promise<{ text: string; language: string }> {
  const videoId = extractVideoId(url);

  const innertubeRes = await fetch(opts.innertubeUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(innertubeBody(videoId)),
  });
  if (!innertubeRes.ok) throw new Error(`innertube ${innertubeRes.status}`);
  const innertube = (await innertubeRes.json()) as any;

  if (innertube.playabilityStatus && innertube.playabilityStatus.status !== "OK") {
    throw new Error(`Video not playable: ${innertube.playabilityStatus.status}`);
  }

  const tracks: CaptionTrack[] =
    innertube?.captions?.playerCaptionsTracklistRenderer?.captionTracks ?? [];
  const { track, fallbackNote } = selectTrack(tracks, language);

  const xmlRes = await fetch(track.baseUrl);
  if (!xmlRes.ok) throw new Error(`caption ${xmlRes.status}`);
  const xml = await xmlRes.text();

  const text = parseTranscriptXml(xml);
  return {
    text: fallbackNote ? `[${fallbackNote}]\n\n${text}` : text,
    language: track.languageCode,
  };
}
