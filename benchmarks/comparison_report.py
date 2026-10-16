"""Raw-sample preservation and deliberately limited descriptive statistics."""
from __future__ import annotations

from collections import defaultdict
import csv
import json
import statistics


def write_report(directory, manifest, samples):
    (directory / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (directory / "samples.json").write_text(json.dumps(samples, indent=2) + "\n")
    fields = ["round", "case", "format", "adapter", "fetch_size", "seconds", "client_cpu_seconds",
              "server_cpu_seconds", "sampled_peak_rss_bytes", "rss_before_query_bytes",
              "process_peak_before_validation_bytes", "error"]
    with (directory / "samples.csv").open("w") as file:
        writer = csv.DictWriter(file, fieldnames=fields, extrasaction="ignore")
        writer.writeheader()
        writer.writerows(samples)
    groups = defaultdict(list)
    for sample in samples:
        groups[(sample["case"], sample["format"], sample["adapter"], sample["fetch_size"])].append(sample)
    lines = ["# Comparação local reproduzível", "", f"Build: `{manifest['build_id']}`.", "",
             "Medianas descritivas; esta amostragem não estima p95/p99. Falhas permanecem no relatório.",
             "RSS é o pico do processo na janela da consulta, incluindo runtime e memória retida pelo warmup.",
             "O baseline `firebird-driver-reset` inclui o mesmo rollback + reset de sessão do fbfast;",
             "`firebird-driver` e `fdb` só fecham cursor e fazem rollback. Nenhum reconecta por consulta.", "",
             "| Caso | Saída | Driver | Fetch | n/erros | Tempo s | Linhas/s | RSS MiB |",
             "|---|---|---|---:|---:|---:|---:|---:|"]
    for (case, kind, adapter, size), group in sorted(groups.items()):
        ok = [row for row in group if "error" not in row]
        errors = len(group) - len(ok)
        if not ok:
            lines.append(f"| {case} | {kind} | {adapter} | {size} | 0/{errors} | — | — | — |")
            continue
        seconds = statistics.median(row["seconds"] for row in ok)
        rss = statistics.median(row["sampled_peak_rss_bytes"] for row in ok) / 2**20
        rows = ok[0]["correctness"]["rows"]
        lines.append(f"| {case} | {kind} | {adapter} | {size} | {len(ok)}/{errors} | "
                     f"{seconds:.6f} | {rows / seconds:.0f} | {rss:.1f} |")
    lines.extend(["", "Limites e procedimento:", "", *[f"- {v}" for v in manifest["limitations"]]])
    (directory / "report.md").write_text("\n".join(lines) + "\n")
