#!/usr/bin/env python3
"""explore.py BIN FIXTURE SIZE W H LABEL STEP...: AT-SPI launch, then steps: click:TEXT (first node
whose name contains TEXT; '=' prefix for a whole name), resize:W:H, wait:S, dump:NAME (nodes to
raw/LABEL-NAME.txt and a screenshot raw/LABEL-NAME.png). Prints each click's point."""
import sys, time; sys.path.insert(0, __import__('os').path.dirname(__file__)); from common import *
b, fx, sz, w, h, label = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5]), sys.argv[6]
L = Launch(b, fx, sz, label, w, h, measured=False); time.sleep(1.5)
for st in sys.argv[7:]:
    k, _, a = st.partition(':')
    if k == 'click':
        p = L.node(a); print('click', a, p, flush=True)
        if p: L.click(*p); time.sleep(1.5)
    elif k == 'resize': W, H = map(int, a.split(':')); L.resize(W, H); time.sleep(1.5)
    elif k == 'wait': time.sleep(float(a))
    elif k == 'dump': L.shot(f'{label}-{a}'); open(f'{TOP}/raw/{label}-{a}.txt', 'w').write(L.nodes())
L.finish()
