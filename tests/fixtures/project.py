#!/usr/bin/env python3
"""Projects a probe run (real responses, already sanitized by the probe script) onto the `Reading`/`Meter`
contract. Makes no network requests and reads no secrets.

Output: a readings JSON file (the same serde format the app (de)serializes in its cache file)
        + a markdown table on stdout (summary of the measurements).

Usage: python3 project.py [probe.json] [output.json]
       (defaults: probe_2026-09-26.json and readings.json, both next to this script)
"""
import datetime as dt
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
INPUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "probe_2026-09-26.json")
OUTPUT = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "readings.json")

# default thresholds: REMAINING fraction
WARNING, CRITICAL = 0.20, 0.05


def level(used, limit, remaining):
    if limit in (None, 0):
        if remaining is not None and remaining <= 0:
            return "exhausted"
        return "no_reference"  # balance without a limit: only gets a level from an absolute threshold in the config
    if remaining is None and used is not None:
        remaining = limit - used
    if remaining is None:
        return "no_reference"
    f = remaining / limit
    if f <= 0:
        return "exhausted"
    if f <= CRITICAL:
        return "critical"
    if f <= WARNING:
        return "warning"
    return "ok"


def meter(label, unit, used=None, limit=None, remaining=None, currency=None, resets_at=None):
    if remaining is None and used is not None and limit is not None:
        remaining = round(limit - used, 6)
    return {"label": label, "unit": unit, "currency": currency, "used": used, "limit": limit,
            "remaining": remaining, "resets_at": resets_at, "level": level(used, limit, remaining)}


def unix_iso(t):
    return dt.datetime.fromtimestamp(int(t), dt.timezone.utc).isoformat().replace("+00:00", "Z")


def iso_norm(s):
    return dt.datetime.fromisoformat(s.replace("Z", "+00:00")).astimezone(dt.timezone.utc).isoformat(
        timespec="seconds").replace("+00:00", "Z") if s else None


# ------------------------------------------------------------------ parsers (mirror `interpret` in Rust)
def p_tavily(r, _):
    a = r["body"]["account"]
    return [meter("monthly plan", "credits", a["plan_usage"], a["plan_limit"])]


def p_firecrawl(r, _):
    d = r["body"]["data"]
    return [meter("credits", "credits", limit=d["planCredits"], remaining=d["remainingCredits"],
                  resets_at=iso_norm(d["billingPeriodEnd"]))]


def p_serper(r, _):
    return [meter("credits", "credits", remaining=r["body"]["balance"])]


def p_brave(r, t0):
    h = r["headers"]
    limits = [int(x) for x in h["x-ratelimit-limit"].split(",")]
    remaining = [int(x) for x in h["x-ratelimit-remaining"].split(",")]
    resets = [int(x) for x in h["x-ratelimit-reset"].split(",")]
    i = len(limits) - 1  # the last window is the monthly one (policy "…, 2000;w=2592000")
    return [meter("requests/month", "requests", limit=limits[i], remaining=remaining[i],
                  resets_at=unix_iso(t0 + resets[i]))]


def p_jina(r, _):
    w = r["body"]["wallet"]
    return [meter("tokens", "tokens", remaining=w["total_balance"])]


def p_openrouter(r, _):
    d = r["body"]["data"]
    return [meter("credits", "currency", d["total_usage"], d["total_credits"], currency="USD")]


def p_opencode(r, _):
    usage = r["body"]["usage"]
    names = {"rolling": "5h", "weekly": "weekly", "monthly": "monthly"}
    return [meter(names[k], "percent", usage[k]["percent"], 100, resets_at=iso_norm(usage[k]["resetsAt"]))
            for k in ("rolling", "weekly", "monthly") if usage.get(k)]


def p_claude(r, _):
    b = r["body"]
    out = []
    for k, name in (("five_hour", "5h"), ("seven_day", "7 days"), ("seven_day_opus", "7 days Opus"),
                    ("seven_day_sonnet", "7 days Sonnet")):
        v = b.get(k)
        if v and v.get("utilization") is not None:
            out.append(meter(name, "percent", v["utilization"], 100, resets_at=iso_norm(v.get("resets_at"))))
    extra = b.get("extra_usage") or {}
    if extra.get("is_enabled"):
        out.append(meter("extra usage", "currency", extra.get("used_credits"), extra.get("monthly_limit"),
                         currency=extra.get("currency")))
    return out


def p_ollama(r, _):
    limits = r["body"]["limits"]
    names = {"session": "session", "weekly": "weekly", "monthly": "monthly"}
    return [meter(names[k], "percent", round(limits[k]["usage"] * 100, 3), 100)
            for k in ("session", "weekly", "monthly") if k in limits]


def p_elevenlabs(r, _):
    b = r["body"]
    return [meter("characters", "characters", b["character_count"], b["character_limit"],
                  resets_at=unix_iso(b["next_character_count_reset_unix"]))]


def p_context7(r, _):
    h = r["headers"]
    return [meter("requests/month", "requests", limit=int(h["ratelimit-limit"]),
                  remaining=int(h["ratelimit-remaining"]), resets_at=unix_iso(h["ratelimit-reset"]))]


def p_twilio(r, _):
    b = r["body"]
    return [meter("balance", "currency", remaining=float(b["balance"]), currency=b["currency"])]


def p_github(r, _):
    core = r["body"]["resources"]["core"]
    return [meter("core/hour", "requests", core["used"], core["limit"], resets_at=unix_iso(core["reset"]))]


def p_validation(r, _):
    return []  # only validates the key: no meter


# probe id -> (provider, service, category, class, cost, parser)
PROVIDERS = {
    "tavily_usage": ("tavily", "Tavily", "search", "usage_vs_limit", "free", p_tavily),
    "firecrawl_v2": ("firecrawl", "Firecrawl", "search", "exact_balance", "free", p_firecrawl),
    "exa_admin_keys": ("exa", "Exa", "search", "no_api", "free", None),
    "serper_account": ("serper", "Serper", "search", "exact_balance", "free", p_serper),
    "brave_search": ("brave", "Brave Search", "search", "rate_limit_only", "per_request", p_brave),
    "jina_fe_user": ("jina", "Jina", "search", "exact_balance", "free", p_jina),
    "openrouter_credits": ("openrouter", "OpenRouter", "llm", "exact_balance", "free", p_openrouter),
    "opencode_go_usage": ("opencode_go", "OpenCode Go", "subscription", "usage_vs_limit", "free", p_opencode),
    "claude_oauth_usage": ("claude", "Claude", "subscription", "usage_vs_limit", "free", p_claude),
    "deepseek_balance": ("deepseek", "DeepSeek", "llm", "exact_balance", "free", None),
    "minimax_token_plan": ("minimax", "MiniMax", "llm", "no_api", "free", None),
    "xai_mgmt_prepaid": ("xai", "xAI", "llm", "exact_balance", "free", None),
    "groq_models": ("groq", "Groq", "llm", "no_api", "free", p_validation),
    "gemini_models": ("gemini", "Gemini", "llm", "no_api", "free", p_validation),
    "mistral_models": ("mistral", "Mistral", "llm", "no_api", "free", p_validation),
    "cerebras_models": ("cerebras", "Cerebras", "llm", "no_api", "free", p_validation),
    "ollama_usage": ("ollama", "Ollama Cloud", "llm", "usage_vs_limit", "free", p_ollama),
    "elevenlabs_sub": ("elevenlabs", "ElevenLabs", "other", "usage_vs_limit", "free", p_elevenlabs),
    "deepgram_projects": ("deepgram", "Deepgram", "other", "exact_balance", "free", None),
    "fal_billing": ("fal", "fal.ai", "other", "exact_balance", "free", None),
    "composio_usage": ("composio", "Composio", "other", "usage_vs_limit", "free", None),
    "context7_search": ("context7", "Context7", "other", "rate_limit_only", "per_request", p_context7),
    "twilio_balance": ("twilio", "Twilio", "other", "exact_balance", "free", p_twilio),
    "pushover_limits": ("pushover", "Pushover", "other", "usage_vs_limit", "free", None),
    "github_rate_limit": ("github", "GitHub", "other", "rate_limit_only", "free", p_github),
    "x_usage_tweets": ("x", "X API", "other", "usage_vs_limit", "free", None),
}


def status_of(probe, r):
    if not probe["credential_present"]:
        return {"kind": "no_credential"}
    h = r.get("http")
    if h is None:
        return {"kind": "network_error", "message": r.get("network_error", "")}
    if h in (401, 403):
        return {"kind": "invalid_credential", "http": h}
    if h == 429:
        return {"kind": "rate_limited", "http": h}
    if h >= 400:
        return {"kind": "api_error", "http": h, "message": json.dumps(r.get("body"), ensure_ascii=False)[:120]}
    return {"kind": "ok"}


def main():
    run = json.load(open(INPUT, encoding="utf-8"))
    t0 = dt.datetime.fromisoformat(run["generated_at"]).timestamp()
    readings, rows = [], []
    for probe in run["probes"]:
        if probe["id"] not in PROVIDERS:
            continue  # auxiliary probes (firecrawl_v1, openrouter_key, xai_api_key) only appear in the summary
        pid, name, category, cls, cost, parse = PROVIDERS[probe["id"]]
        r = probe["response"]
        status = status_of(probe, r)
        meters = []
        if status["kind"] == "ok":
            if parse is None:
                status = {"kind": "unsupported"}  # e.g. Exa with the regular key
            else:
                try:
                    meters = parse(r, t0)
                except (KeyError, TypeError, ValueError) as e:
                    status = {"kind": "unexpected_format", "message": f"{type(e).__name__}: {e}"}
        readings.append({"provider": pid, "service": name, "category": category, "class": cls, "cost": cost,
                         "read_at": run["generated_at"], "duration_ms": r.get("ms"), "status": status,
                         "meters": meters})
        summary = "; ".join(
            f"{m['label']}: " + (f"{m['used']}/{m['limit']}" if m["used"] is not None
                                 else f"{m['remaining']} left" + (f"/{m['limit']}" if m["limit"] else ""))
            + (f" {m['currency']}" if m["currency"] else "") + f" [{m['level']}]" for m in meters) or "—"
        rows.append(f"| {name} | {cls} | {status['kind']} | {r.get('http')} | {r.get('ms')} | {summary} |")
    doc = {"version": 2, "generated_at": run["generated_at"], "source": os.path.basename(INPUT),
           "readings": readings}
    with open(OUTPUT, "w", encoding="utf-8") as fh:
        json.dump(doc, fh, ensure_ascii=False, indent=1)
    print("| Service | Class | Status | HTTP | ms | Projected meters |")
    print("|---|---|---|---|---|---|")
    print("\n".join(rows))
    ms = [p["response"].get("ms") or 0 for p in run["probes"] if p["id"] in PROVIDERS]
    print(f"\nlatency of the {len(ms)} mapped probes: sum={sum(ms)} ms (sequential), max={max(ms)} ms (parallel)")
    print(f"written: {OUTPUT} ({len(readings)} readings)")


if __name__ == "__main__":
    main()
