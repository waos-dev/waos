"""Writes Model Viewer's icon.svg: a flat-shaded icosphere lit from above, on a light tile.

The facets are computed here because no one places 80 triangles by hand:
`python3 icon.py icon.svg` writes the SVG, from which icon.png is rendered.
"""
import math, sys

def icosphere(subdiv):
    t = (1 + 5 ** 0.5) / 2
    v = [(-1, t, 0), (1, t, 0), (-1, -t, 0), (1, -t, 0), (0, -1, t), (0, 1, t), (0, -1, -t), (0, 1, -t), (t, 0, -1), (t, 0, 1), (-t, 0, -1), (-t, 0, 1)]
    v = [norm(p) for p in v]
    f = [(0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11), (1, 5, 9), (5, 11, 4), (11, 10, 2), (10, 7, 6), (7, 1, 8), (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8), (3, 8, 9), (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1)]
    for _ in range(subdiv):
        cache = {}
        def mid(a, b):
            k = tuple(sorted((a, b)))
            if k not in cache:
                cache[k] = len(v)
                v.append(norm(tuple((v[a][i] + v[b][i]) / 2 for i in range(3))))
            return cache[k]
        nf = []
        for a, b, c in f:
            ab, bc, ca = mid(a, b), mid(b, c), mid(c, a)
            nf += [(a, ab, ca), (b, bc, ab), (c, ca, bc), (ab, bc, ca)]
        f = nf
    return v, f

def norm(p):
    l = math.sqrt(sum(x * x for x in p))
    return tuple(x / l for x in p)

def rotate(p, ax, ay):
    x, y, z = p
    x, z = x * math.cos(ay) + z * math.sin(ay), -x * math.sin(ay) + z * math.cos(ay)
    y, z = y * math.cos(ax) - z * math.sin(ax), y * math.sin(ax) + z * math.cos(ax)
    return (x, y, z)

def mix(c1, c2, t):
    return tuple(round(c1[i] + (c2[i] - c1[i]) * t) for i in range(3))

def facets(cx, cy, r, dark, light, spec):
    v, f = icosphere(1)
    v = [rotate(p, 0.35, 0.3) for p in v]
    lightdir = norm((-0.45, 0.8, 0.55))  # y up in model space
    out = []
    for a, b, c in f:
        pa, pb, pc = v[a], v[b], v[c]
        n = norm(tuple(sum(q[i] for q in (pa, pb, pc)) for i in range(3)))
        if n[2] <= 0:
            continue
        d = max(0, sum(n[i] * lightdir[i] for i in range(3)))
        col = mix(dark, light, 0.15 + 0.85 * d)
        h = norm((lightdir[0], lightdir[1], lightdir[2] + 1))
        s = max(0, sum(n[i] * h[i] for i in range(3))) ** 40
        col = mix(col, spec, min(1, s * 0.9))
        pts = " ".join(f"{cx + p[0] * r:.1f},{cy - p[1] * r:.1f}" for p in (pa, pb, pc))
        hexc = "#%02x%02x%02x" % col
        out.append(f'<polygon points="{pts}" fill="{hexc}" stroke="{hexc}" stroke-width="1.5" stroke-linejoin="round"/>')
    return "\n    ".join(out)

def grid(gx, gy):
    """A ground plane's grid seen from slightly above: rows nearer the eye wider apart, columns meeting far behind."""
    lines = []
    for k in range(-2, 4):
        y = gy + k * 30 + (k * abs(k)) * 5
        lines.append(f'<path d="M{gx - 420} {y:.1f} H{gx + 420}"/>')
    for k in range(-5, 6):
        x0 = gx + k * 110
        lines.append(f'<path d="M{gx + (x0 - gx) * 0.55:.1f} {gy - 120} L{x0 + (x0 - gx) * 0.6:.1f} {gy + 160}"/>')
    return "\n    ".join(lines)

def hexrgb(h):
    return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))

if __name__ == "__main__":
    cx, cy, r = 512, 466, 282
    body = facets(cx, cy, r, hexrgb("#2b1f8f"), hexrgb("#8f86ff"), hexrgb("#ffffff"))
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <!-- Model Viewer: a faceted sphere, flat-shaded and lit from above, standing on a viewport's ground grid. Written by icon.py, which computes the facets and their shading: change it and run `python3 icon.py icon.svg` rather than editing this file. -->
  <defs>
    <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffffff"/><stop offset="1" stop-color="#d9dbe2"/></linearGradient>
    <radialGradient id="contact" cx="0.5" cy="0.5" r="0.5"><stop offset="0" stop-color="#1b1d2b" stop-opacity="0.45"/><stop offset="1" stop-color="#1b1d2b" stop-opacity="0"/></radialGradient>
    <radialGradient id="gridfade" cx="0.5" cy="0.5" r="0.5"><stop offset="0.35" stop-color="#fff"/><stop offset="1" stop-color="#000"/></radialGradient>
    <mask id="gridmask"><ellipse cx="{cx}" cy="{cy + r + 30}" rx="360" ry="110" fill="url(#gridfade)"/></mask>
  </defs>
  <rect width="1024" height="1024" fill="url(#tile)"/>
  <g mask="url(#gridmask)" stroke="#8a90a6" stroke-width="5" fill="none" stroke-opacity="0.6">
    {grid(cx, cy + r + 30)}
  </g>
  <ellipse cx="{cx}" cy="{cy + r + 18}" rx="230" ry="40" fill="url(#contact)"/>
  <g>
    {body}
  </g>
</svg>
'''
    open(sys.argv[1], "w").write(svg)
