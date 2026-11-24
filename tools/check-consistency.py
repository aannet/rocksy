#!/usr/bin/env python3
"""Consistency checks for the Rocksy repository (Python 3, standard library only).

Run from the repository root:  python3 tools/check-consistency.py
Exit code 0 when everything passes, 1 on any failure.

Checks
  1. Command surface: the same set of IPC commands in app/CONTRACT.md (commands table),
     generate_handler! (main.rs), build.rs, capabilities/default.json, the Tauri implementation of
     backend.ts and the mock backend.
  2. Doc links: every repository path written in backticks in AGENTS.md, docs/** (except
     docs/archive/) and app/CONTRACT.md exists (or is ignored by git).
  3. Doc headers: normative and living documents carry `Status:` and `Last verified at:`; ADRs carry
     `Status:` and `Date:` and their three sections.
  4. Privacy: if an untracked `.private-terms` file exists (one term per line, `#` comments allowed),
     no tracked file may contain any term. Terms are never printed, only their line number in the file.
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
os.chdir(ROOT)

failures = []


def fail(check, msg):
    failures.append((check, msg))
    print(f"  FAIL [{check}] {msg}")


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def git(*args):
    r = subprocess.run(["git", *args], capture_output=True, text=True, cwd=ROOT)
    return r.returncode, r.stdout


# ---------------------------------------------------------------- 1. command surface

IDENT = re.compile(r"^[a-z][a-z0-9_]*$")


def contract_commands():
    text = read("app/CONTRACT.md")
    m = re.search(r"^## Commands.*?\n(.*?)(?=^## )", text, re.S | re.M)
    if not m:
        raise ValueError("section '## Commands' not found in app/CONTRACT.md")
    names = set()
    for line in m.group(1).splitlines():
        if not line.startswith("|"):
            continue
        first = line.split("|")[1]
        for tok in re.findall(r"`([^`]+)`", first):
            if IDENT.match(tok):
                names.add(tok)
    return names


def handler_commands():
    text = read("app/src-tauri/src/main.rs")
    m = re.search(r"generate_handler!\s*\[(.*?)\]", text, re.S)
    if not m:
        raise ValueError("generate_handler! not found in main.rs")
    return {t.strip() for t in m.group(1).split(",") if t.strip()}


def build_rs_commands():
    text = read("app/src-tauri/build.rs")
    m = re.search(r"const COMMANDS[^=]*=\s*&\[(.*?)\];", text, re.S)
    if not m:
        raise ValueError("COMMANDS array not found in build.rs")
    return set(re.findall(r'"([a-z0-9_]+)"', m.group(1)))


def capability_commands():
    data = json.loads(read("app/src-tauri/capabilities/default.json"))
    return {p[len("allow-"):].replace("-", "_") for p in data["permissions"] if p.startswith("allow-")}


def backend_ts_commands():
    text = read("app/ui/src/backend.ts")
    m = re.search(r"function tauriBackend\(\).*?\n}\n", text, re.S)
    body = m.group(0) if m else text
    return set(re.findall(r'\bcall(?:<[^>]*>)?\(\s*"([a-z0-9_]+)"', body))


def camel_to_snake(s):
    return re.sub(r"([A-Z])", lambda mm: "_" + mm.group(1).lower(), s)


def mock_commands():
    text = read("app/ui/src/mock/mock.ts")
    i = text.index("const backend: Backend")
    body = text[i:]
    names = set(re.findall(r"^    async ([A-Za-z0-9]+)\(", body, re.M))
    return {camel_to_snake(n) for n in names}


def check_command_surface():
    print("1. Command surface")
    try:
        sources = {
            "CONTRACT.md": contract_commands(),
            "generate_handler!": handler_commands(),
            "build.rs": build_rs_commands(),
            "capabilities/default.json": capability_commands(),
            "backend.ts": backend_ts_commands(),
            "mock": mock_commands(),
        }
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as e:
        fail("commands", f"cannot read a source: {e}")
        return
    union = set().union(*sources.values())
    for name, got in sources.items():
        missing = sorted(union - got)
        if missing:
            fail("commands", f"{name} lacks: {', '.join(missing)}")
    if not any(c == "commands" for c, _ in failures):
        print(f"  ok: {len(union)} commands identical in {len(sources)} places")


# ---------------------------------------------------------------- 2. doc links

PATH_RE = re.compile(r"^(?:(?:app|docs|tools|release|runs|data|in)/[A-Za-z0-9_./-]*|AGENTS\.md|CLAUDE\.md)$")
# untracked on purpose (kept on disk only): not expected in a fresh clone
UNTRACKED_ON_PURPOSE = {"in/audit-vault-obsidian.md", "in/audit-vault-obsidian.json"}


def doc_files_for_links():
    files = ["AGENTS.md", "app/CONTRACT.md"]
    for base, dirs, names in os.walk("docs"):
        if base.replace(os.sep, "/").startswith("docs/archive"):
            dirs[:] = []
            continue
        for n in names:
            if n.endswith(".md"):
                files.append(os.path.join(base, n).replace(os.sep, "/"))
    return sorted(files)


def path_ok(p):
    if p in UNTRACKED_ON_PURPOSE:
        return True
    if os.path.exists(p):
        return True
    rc, _ = git("check-ignore", "-q", p)
    return rc == 0


def check_doc_links():
    print("2. Doc links")
    n = 0
    bad = 0
    for f in doc_files_for_links():
        in_fence = False
        for ln, line in enumerate(read(f).splitlines(), 1):
            if line.lstrip().startswith("```"):
                in_fence = not in_fence
                continue
            if in_fence:
                continue
            for tok in re.findall(r"`([^`\n]+)`", line):
                if PATH_RE.match(tok):
                    n += 1
                    if not path_ok(tok):
                        bad += 1
                        fail("links", f"{f}:{ln}: `{tok}` does not exist")
    if not bad:
        print(f"  ok: {n} paths checked")


# ---------------------------------------------------------------- 3. headers

STATUS_RE = re.compile(r"^Status:\s*(normative|living|historical)\s*$", re.M)
VERIFIED_RE = re.compile(r"^Last verified at:\s*\S+", re.M)


def header_files():
    files = ["AGENTS.md", "app/CONTRACT.md", "release/README.md"]
    for d in ("docs", "docs/specs", "docs/runbooks", "docs/handoffs"):
        for n in sorted(os.listdir(d)):
            p = f"{d}/{n}"
            if n.endswith(".md") and os.path.isfile(p):
                files.append(p)
    return files


def check_headers():
    print("3. Doc headers")
    bad = 0
    files = header_files()
    for f in files:
        head = "\n".join(read(f).splitlines()[:12])
        if not STATUS_RE.search(head):
            bad += 1
            fail("headers", f"{f}: missing `Status: normative|living|historical` in the first 12 lines")
        if not VERIFIED_RE.search(head):
            bad += 1
            fail("headers", f"{f}: missing `Last verified at:` in the first 12 lines")
    adrs = sorted(n for n in os.listdir("docs/decisions") if re.match(r"\d{4}-.*\.md$", n))
    if not adrs:
        fail("headers", "docs/decisions has no ADR")
    for n in adrs:
        f = f"docs/decisions/{n}"
        text = read(f)
        head = "\n".join(text.splitlines()[:8])
        if not re.search(r"^Status:\s*(accepted|superseded by \d{4})\s*$", head, re.M):
            bad += 1
            fail("headers", f"{f}: missing `Status: accepted` or `Status: superseded by NNNN`")
        if not re.search(r"^Date:\s*\d{4}-\d{2}-\d{2}\s*$", head, re.M):
            bad += 1
            fail("headers", f"{f}: missing `Date: YYYY-MM-DD`")
        for sec in ("## Context", "## Decision", "## Consequences"):
            if sec not in text:
                bad += 1
                fail("headers", f"{f}: missing section `{sec}`")
    if not bad:
        print(f"  ok: {len(files)} documents and {len(adrs)} ADRs")


# ---------------------------------------------------------------- 4. privacy


def check_privacy():
    print("4. Privacy")
    p = ".private-terms"
    if not os.path.isfile(p):
        print("  skipped: no .private-terms file (create one at the repository root, one term per line; it is git-ignored)")
        return
    terms = []
    for ln, line in enumerate(read(p).splitlines(), 1):
        t = line.strip()
        if t and not t.startswith("#"):
            terms.append((ln, t.lower()))
    if not terms:
        print("  skipped: .private-terms holds no term")
        return
    rc, out = git("ls-files", "--cached", "--others", "--exclude-standard", "-z")
    if rc != 0:
        fail("privacy", "git ls-files failed")
        return
    bad = 0
    nfiles = 0
    for f in [x for x in out.split("\0") if x]:
        if f == p or not os.path.isfile(f):
            continue
        nfiles += 1
        low_name = f.lower()
        try:
            with open(f, "rb") as fh:
                data = fh.read().decode("utf-8", errors="ignore").lower()
        except OSError:
            continue
        for ln, t in terms:
            if t in low_name:
                bad += 1
                fail("privacy", f"{f}: file name contains the term of .private-terms line {ln}")
            if t in data:
                bad += 1
                line_no = data[: data.index(t)].count("\n") + 1
                fail("privacy", f"{f}:{line_no}: contains the term of .private-terms line {ln}")
    if not bad:
        print(f"  ok: {len(terms)} terms, {nfiles} files")


def main():
    check_command_surface()
    check_doc_links()
    check_headers()
    check_privacy()
    print()
    if failures:
        print(f"FAILED: {len(failures)} problem(s)")
        return 1
    print("OK: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
