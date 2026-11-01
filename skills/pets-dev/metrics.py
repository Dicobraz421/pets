"""Метрики процесса pets-dev по GitHub: PR, ручные вмешательства, время до мержа, красные проверки.

    python metrics.py <owner/repo> --since 2026-10-30 [--until 2026-11-01] [--cut 2026-10-31T09:40Z]

`--cut` делит выборку на «до» и «после» (смена правил). Вывод — Markdown для `docs/metrics/`.
Нужен `gh` с доступом к репозиторию. Токены, время агентов и диск GitHub не знает — их пишет
оркестратор в журнал волн (скилл pets-dev, «Замер»).
"""

import argparse
import json
import statistics
import subprocess
import sys
from collections import Counter
from datetime import datetime, timezone

QUERY = """
query($q: String!, $cursor: String) {
  search(type: ISSUE, query: $q, first: 50, after: $cursor) {
    pageInfo { hasNextPage endCursor }
    nodes { ... on PullRequest {
      number createdAt mergedAt state headRefName
      commits(first: 100) { nodes { commit {
        parents { totalCount }
        statusCheckRollup { contexts(first: 50) { nodes {
          ... on CheckRun { name conclusion }
        } } }
      } } }
      timelineItems(first: 100, itemTypes: [BASE_REF_CHANGED_EVENT, REOPENED_EVENT]) {
        nodes { __typename }
      }
    } }
  }
}
"""


def ts(s: str) -> datetime:
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def fetch(repo: str, since: str, until: str | None) -> list[dict]:
    q = f"repo:{repo} is:pr created:{since}..{until or '*'}"
    prs, cursor = [], None
    while True:
        args = ["gh", "api", "graphql", "-f", f"query={QUERY}", "-F", f"q={q}"]
        if cursor:
            args += ["-F", f"cursor={cursor}"]
        out = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", check=True).stdout
        page = json.loads(out)["data"]["search"]
        prs += [n for n in page["nodes"] if n]
        if not page["pageInfo"]["hasNextPage"]:
            return prs
        cursor = page["pageInfo"]["endCursor"]


def summary(prs: list[dict]) -> dict:
    code = [p for p in prs if p["headRefName"].startswith("w/") or "-dep" in p["headRefName"]]
    fails: Counter = Counter()
    for p in prs:
        for c in p["commits"]["nodes"]:
            roll = c["commit"]["statusCheckRollup"]
            for ctx in (roll or {}).get("contexts", {}).get("nodes", []):
                if ctx.get("conclusion") == "FAILURE" and ctx.get("name") != "gate":
                    fails[ctx["name"]] += 1
    events = Counter(e["__typename"] for p in prs for e in p["timelineItems"]["nodes"])
    hours = [(ts(p["mergedAt"]) - ts(p["createdAt"])).total_seconds() / 60 for p in code if p["mergedAt"]]
    return {
        "PR": len(prs),
        "из них с кодом": len(code),
        "закрыты без мержа (пересоздание)": sum(p["state"] == "CLOSED" for p in prs),
        "обновлений ветки (merge-коммит в PR)": sum(
            any(c["commit"]["parents"]["totalCount"] > 1 for c in p["commits"]["nodes"]) for p in code),
        "смен базы PR": events["BaseRefChangedEvent"],
        "переоткрытий (перезапуск CI)": events["ReopenedEvent"],
        "медиана открытие → мерж, мин": round(statistics.median(hours)) if hours else "—",
        "красные проверки": ", ".join(f"{k} {v}" for k, v in fails.most_common()) or "нет",
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("repo")
    ap.add_argument("--since", required=True)
    ap.add_argument("--until")
    ap.add_argument("--cut")
    a = ap.parse_args()
    prs = fetch(a.repo, a.since, a.until)
    parts = {"все": prs}
    if a.cut:
        cut = ts(a.cut).astimezone(timezone.utc)
        parts = {"до": [p for p in prs if ts(p["createdAt"]) < cut],
                 "после": [p for p in prs if ts(p["createdAt"]) >= cut]}
    cols = {k: summary(v) for k, v in parts.items()}
    keys = list(next(iter(cols.values())))
    sys.stdout.reconfigure(encoding="utf-8")
    print(f"## GitHub: {a.repo}, {a.since} … {a.until or 'сейчас'}\n")
    print("| Показатель | " + " | ".join(cols) + " |")
    print("|---|" + "---|" * len(cols))
    for k in keys:
        print(f"| {k} | " + " | ".join(str(c[k]) for c in cols.values()) + " |")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
