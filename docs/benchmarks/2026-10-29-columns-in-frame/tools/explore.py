#!/usr/bin/env python3
"""explore.py BIN FIXTURE SIZE W H LABEL STEP...: an unmeasured launch (NO_AT_BRIDGE=1) that runs
STEPs and screenshots after each to raw/x/LABEL-N.png. STEP: k:KEY (wtype -k), h:KEY:MS (hold),
c:X:Y (click, window-relative logical px), w:N (wheel at the pointer), m:X:Y, r:W (resize width),
s:SEC (sleep)."""
import os, sys, time, subprocess; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); from common import *
binary, fixture, size, w, h, label = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5]), sys.argv[6]
os.makedirs(f'{TOP}/raw/x', exist_ok=True)
L = Launch(binary, fixture, size, f'x/{label}', w, h, measured=True)
L.shot(f'x/{label}-0')
for n, st in enumerate(sys.argv[7:], 1):
    a = st.split(':')
    if a[0] == 'k': L.key('-k', a[1])
    elif a[0] == 'W': subprocess.run(['wtype', *a[1].split()], check=True)
    elif a[0] == 'h': subprocess.run(['wtype', '-P', a[1], '-s', a[2], '-p', a[1]], check=True)
    elif a[0] == 'c': L.click(int(a[1]), int(a[2]))
    elif a[0] == 'm': L.move(int(a[1]), int(a[2]))
    elif a[0] == 'w': subprocess.run([VPTR, 'stdin'], input=f'w {a[1]}\nq\n', text=True, check=True, capture_output=True)
    elif a[0] == 'r': L.resize(int(a[1]), h)
    elif a[0] == 's': time.sleep(float(a[1]))
    time.sleep(1.2); L.shot(f'x/{label}-{n}')
L.finish(); print('done', L.marks['exit'])
