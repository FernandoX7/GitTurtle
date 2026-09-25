/* ncc.c: zero-mean normalized cross-correlation of grayscale templates over a grayscale frame.
   Built on demand by privacy.py into a cache directory outside the repository.
   usage: ncc FRAME.raw W H THRESH TPL1.raw TW1 TH1 [TPL2.raw TW2 TH2 ...]
   prints, per template: "tpl <i> best <|ncc|> <x> <y> sign <+/-> hits <n>" and up to 20 "hit <x> <y> <ncc>"
   lines (|ncc| >= THRESH, non-maximum suppressed by template size). Flat windows (stddev < 4) are skipped;
   privacy.py's pure-Python engine implements the same rules. */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>

static unsigned char *load(const char *path, long n) {
  FILE *f = fopen(path, "rb");
  if (!f) { perror(path); exit(2); }
  unsigned char *b = malloc(n);
  if (!b || fread(b, 1, n, f) != (size_t)n) { fprintf(stderr, "short %s\n", path); exit(2); }
  fclose(f);
  return b;
}

int main(int argc, char **argv) {
  if (argc < 8 || (argc - 5) % 3) { fprintf(stderr, "usage: ncc FRAME W H THRESH TPL TW TH ...\n"); return 2; }
  int W = atoi(argv[2]), H = atoi(argv[3]);
  double th = atof(argv[4]);
  if (W <= 0 || H <= 0) { fprintf(stderr, "bad frame size\n"); return 2; }
  unsigned char *F = load(argv[1], (long)W * H);
  double *S = calloc((size_t)(W + 1) * (H + 1), sizeof(double));
  double *Q = calloc((size_t)(W + 1) * (H + 1), sizeof(double));
  float *Ff = malloc(sizeof(float) * (size_t)W * H);
  if (!S || !Q || !Ff) { fprintf(stderr, "out of memory\n"); return 2; }
  for (int y = 0; y < H; y++)
    for (int x = 0; x < W; x++) {
      double v = F[(long)y * W + x];
      long i = (long)(y + 1) * (W + 1) + x + 1;
      S[i] = v + S[i - 1] + S[i - (W + 1)] - S[i - (W + 1) - 1];
      Q[i] = v * v + Q[i - 1] + Q[i - (W + 1)] - Q[i - (W + 1) - 1];
    }
  for (long i = 0; i < (long)W * H; i++) Ff[i] = F[i];
  for (int a = 5, k = 0; a < argc; a += 3, k++) {
    int tw = atoi(argv[a + 1]), thh = atoi(argv[a + 2]);
    if (tw <= 0 || thh <= 0) { fprintf(stderr, "bad template size\n"); return 2; }
    int n = tw * thh;
    unsigned char *Tb = load(argv[a], n);
    float *T = malloc(sizeof(float) * n);
    double m = 0, tv = 0;
    for (int i = 0; i < n; i++) m += Tb[i];
    m /= n;
    for (int i = 0; i < n; i++) { T[i] = Tb[i] - m; tv += T[i] * T[i]; }
    double tn = sqrt(tv), best = 0;
    int bx = -1, by = -1, bs = 0, cap = 4096, nh = 0;
    int *hx = malloc(sizeof(int) * cap), *hy = malloc(sizeof(int) * cap);
    double *hv = malloc(sizeof(double) * cap);
    for (int y = 0; tn > 0 && y + thh <= H; y++)
      for (int x = 0; x + tw <= W; x++) {
        long i0 = (long)y * (W + 1) + x, i1 = (long)(y + thh) * (W + 1) + x;
        double s = S[i1 + tw] - S[i1] - S[i0 + tw] + S[i0];
        double q = Q[i1 + tw] - Q[i1] - Q[i0 + tw] + Q[i0];
        double var = q - s * s / n;
        if (var < 16.0 * n) continue;
        double acc = 0;
        for (int ty = 0; ty < thh; ty++) {
          const float *fr = Ff + (long)(y + ty) * W + x, *tr = T + ty * tw;
          float r = 0;
          for (int tx = 0; tx < tw; tx++) r += fr[tx] * tr[tx];
          acc += r;
        }
        double c = acc / (sqrt(var) * tn), ac = fabs(c);
        if (ac > best) { best = ac; bx = x; by = y; bs = c < 0; }
        if (ac >= th) {
          int merged = 0;
          for (int j = 0; j < nh; j++)
            if (abs(hx[j] - x) < tw && abs(hy[j] - y) < thh) {
              merged = 1;
              if (ac > fabs(hv[j])) { hx[j] = x; hy[j] = y; hv[j] = c; }
              break;
            }
          if (!merged && nh < cap) { hx[nh] = x; hy[nh] = y; hv[nh] = c; nh++; }
        }
      }
    printf("tpl %d best %.4f %d %d sign %c hits %d\n", k, best, bx, by, bs ? '-' : '+', nh);
    for (int j = 0; j < nh && j < 20; j++) printf("hit %d %d %.4f\n", hx[j], hy[j], hv[j]);
    free(T); free(Tb); free(hx); free(hy); free(hv);
  }
  return 0;
}
