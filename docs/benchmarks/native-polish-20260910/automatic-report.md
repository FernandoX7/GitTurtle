# Native session evidence draft

Source: `4ebe8fc834f7c03674828407e78ece29c6aa4d9c`; PID `34554`; executable SHA-256 `1fc08c8560041f9232b0c9cad824dfd9488bf904343b981b19c1e532b80d6503`. Source identity is declared by the build owner and checked against the observer header.

Operator-declared mixed review intervals: 1318.0995919704437 seconds. Native action records must establish the exercised workflows and result.


Status: completed observer log; 274 samples over 1365.005s.
Failure trace records: 0. Parser/consistency warnings: 48.

| Resource | Median | p95 | Maximum | First → last |
| --- | ---: | ---: | ---: | ---: |
| rss_mib | 154.430 | 235.062 | 586.188 | 116.875 → 235.062 |
| cpu_percent_interval | 1.800 | 5.800 | 13.802 | 2.997 → 0.800 |
| threads | 15.000 | 26.000 | 26.000 | 14.000 → 20.000 |
| numeric_fd_fresh | 12.000 | 21.000 | 21.000 | 9.000 → 15.000 |
| git_children | 3.000 | 7.000 | 7.000 | 1.000 → 4.000 |
| collection_ms | 67.808 | 123.904 | 153.393 | 96.327 → 77.358 |

| Trace metric | n | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| commit_files_frame_ms | 7 | 37.233 | 51.582 | 51.582 |
| file_preview_frame_ms | 8 | 5.659 | 541.912 | 541.912 |
| history_page_frame_ms | 1 | 7.678 | 7.678 | 7.678 |

Lifecycle: 30 tracked images / 30 frames; 22 retired images / 22 frames; last retained images 8; 0 window-close records.

| Phase / cycle | Marker duration, s | Resource samples | RSS first → last, MiB | Trace bytes | Retired images / frames |
| --- | ---: | ---: | ---: | --- | ---: |
| pr-review / None | 191.873 | 38 | 120.312 → 136.969 | 155–155 | 0 / 0 |
| themes / None | 483.795 | 97 | 136.688 → 154.234 | 155–5307 | 0 / 0 |
| markdown-tab / None | 56.858 | 11 | 154.469 → 165.672 | 5307–5533 | 0 / 0 |
| markdown-link-open / 1 | 23.175 | 5 | 165.672 → 171.828 | 5533–5592 | 3 / 3 |
| markdown-settled / 1 | 47.478 | 9 | 171.828 → 171.828 | 5592–5592 | 0 / 0 |
| markdown-correction / None | 102.003 | 21 | 171.828 → 172.062 | 5592–5817 | 0 / 0 |
| markdown-link-open / 2 | 7.038 | 1 | 172.109 → 172.109 | 5817–5817 | 0 / 0 |
| markdown-settled / 2 | 37.432 | 8 | 172.328 → 171.484 | 5817–5817 | 0 / 0 |
| markdown-link-open / 3 | 6.597 | 1 | 171.547 → 171.547 | 5817–5817 | 0 / 0 |
| markdown-settled / 3 | 26.977 | 5 | 171.547 → 171.500 | 5817–5817 | 0 / 0 |
| markdown-link-open / 4 | 6.557 | 2 | 171.500 → 171.547 | 5817–5817 | 0 / 0 |
| markdown-settled / 4 | 49.468 | 10 | 171.547 → 171.500 | 5817–5817 | 0 / 0 |
| pdf-preview / None | 57.534 | 11 | 171.562 → 189.906 | 5817–6090 | 0 / 0 |
| model-preview / None | 51.124 | 10 | 190.047 → 222.406 | 6090–7711 | 16 / 16 |
| deep-history / None | 151.248 | 31 | 230.453 → 232.875 | 7711–8052 | 0 / 0 |
| cleanup / None | 18.941 | 3 | 234.125 → 234.031 | 8052–8111 | 3 / 3 |
| final-idle / None | 50.798 | 10 | 235.141 → 235.062 | 8111–8111 | 0 / 0 |

Worker-only duration distributions: unavailable unless present as separate named trace metrics; frame callbacks are not worker timings.

Full identities, hashes, raw values, phase endpoints, boundary crossings, errors and limitations are in summary.json. This generated report does not declare a native stability pass.
