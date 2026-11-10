"""The app's icon, from one geometry: the ring, simplified to read at launcher
sizes, on a teal tile. It is the sibling of Havne's tachometer: the same tile
and face, but where the gauge reads the car itself, the ring looks outward
from the vehicle at its center to the emitters around it.

    python3 packaging/icons/make_icons.py

Writes, from the repository root:
  packaging/icons/app.svg        the master: the tile with rounded corners
  android/app/src/main/res/...   the adaptive launcher icon as vector
                                 drawables (sharp at every density), with
                                 a monochrome layer for themed icons

The geometry lives on Android's 108-unit adaptive canvas: launchers show its
middle 72 units, and the face fits the 66-unit safe circle. The ring follows
RingView: bands at the same fractions of the radius (strongest innermost),
sectors laid clockwise from west in the same order and weights, and the same
colors per kind. The dots stand for no real devices.
"""
import math
import os

BG, ELEVATED = "#141413", "#292926"
TEXT, WHITE = "#ecebe5", "#ffffff"
RING, SECTOR = "#908f87", "#5f5e58"   # RingView's ring and sector grays, lifted to read small
TILE_FROM, TILE_TO = "#2fd3bf", "#0b6a64"   # teal, lit from the top left

# RingView's colors per kind, and the tracker's
KIND = {"ble": "#50c878", "bt_classic": "#f0b43c", "access_point": "#5aa0ff",
        "station": "#50dce6"}
TRACKER = "#ff5050"

CX = CY = 54.0
R = 33.0                         # the face
RR = R * 0.88                    # the ring's radius inside it
BANDS = (0.32, 0.56, 0.80)       # RingView.BANDS' solid rings; the outermost is dashed
WEIGHTS = (("ble", 180), ("bt_classic", 40), ("access_point", 110), ("station", 40))

# The dots: kind, angle (degrees, 0 = east, clockwise) and radius as a
# fraction of RR, each inside its kind's sector. The tracker sits up and to
# the left, mirroring the tachometer's needle, which points up and to the right.
DOTS = (
    ("ble", 204, 0.66), ("ble", 290, 0.90), ("ble", 304, 0.62), ("ble", 328, 0.36),
    ("bt_classic", 2, 0.70),
    ("access_point", 44, 0.64), ("access_point", 92, 0.44),
    ("station", 160, 0.50),
)
TRACKER_AT = (256.5, 0.45)

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
RES = "android/app/src/main/res"


def pt(a, r):
    t = math.radians(a)
    return CX + r * math.cos(t), CY + r * math.sin(t)


def n(v):
    return f"{v:.3f}".rstrip("0").rstrip(".")


def arc(a0, a1, r):
    (x0, y0), (x1, y1) = pt(a0, r), pt(a1, r)
    large = 1 if (a1 - a0) % 360 > 180 else 0
    return f"M{n(x0)},{n(y0)} A{n(r)},{n(r)} 0 {large} 1 {n(x1)},{n(y1)}"


def circle(r, cx=CX, cy=CY):
    return f"M{n(cx - r)},{n(cy)} a{n(r)},{n(r)} 0 1,0 {n(2 * r)},0 a{n(r)},{n(r)} 0 1,0 {n(-2 * r)},0 Z"


def sector_edges():
    """Each sector's starting angle, clockwise from west, as RingView lays them"""
    total = sum(w for _, w in WEIGHTS)
    edges, a = [], 180.0
    for _, w in WEIGHTS:
        edges.append(a % 360)
        a += 360 * w / total
    return edges


# The face's light, from above
FACE = {"type": "radial", "cx": CX, "cy": CY - R * 0.35, "r": R * 1.25,
        "stops": [(0, ELEVATED), (1, BG)]}


def ring(mono=False):
    """The ring as paths: dicts of d, fill or stroke (color or gradient),
    width, alpha and cap. With mono, one color for a themed icon's mask
    (Android tints it and reads only its alpha)"""
    c = (lambda color: WHITE) if mono else (lambda color: color)
    out = [] if mono else [{"d": circle(R), "fill": FACE, "stroke": TEXT, "stroke_alpha": 0.14, "width": 0.8}]
    for a in sector_edges():
        (x0, y0), (x1, y1) = pt(a, RR * 0.12), pt(a, RR)
        out.append({"d": f"M{n(x0)},{n(y0)} L{n(x1)},{n(y1)}", "stroke": c(SECTOR),
                    "stroke_alpha": 0.25 if mono else 0.5, "width": 0.8, "cap": "round"})
    for f in BANDS:
        out.append({"d": circle(RR * f), "stroke": c(RING), "stroke_alpha": 0.6 if mono else 0.75, "width": 1.5})
    # The outermost band, "no dBm", dashed as in the app
    dashes = 16
    for k in range(dashes):
        a0 = k * 360 / dashes
        out.append({"d": arc(a0, a0 + 360 / dashes * 0.55, RR), "stroke": c(RING),
                    "stroke_alpha": 0.6 if mono else 0.75, "width": 1.5, "cap": "round"})
    for kind, a, f in DOTS:
        x, y = pt(a, RR * f)
        out.append({"d": circle(2.7, x, y), "fill": c(KIND[kind])})
    x, y = pt(TRACKER_AT[0], RR * TRACKER_AT[1])
    out.append({"d": circle(6.0, x, y), "stroke": c(TRACKER), "stroke_alpha": 0.55, "width": 1.5})
    out.append({"d": circle(3.6, x, y), "fill": c(TRACKER)})
    # The vehicle at the center
    out.append({"d": circle(3.4), "fill": WHITE})
    return out


def tile(rounded):
    rx = 16 if rounded else 0
    d = (f"M{18 + rx},18 H{90 - rx} A{rx},{rx} 0 0 1 90,{18 + rx} V{90 - rx} "
         f"A{rx},{rx} 0 0 1 {90 - rx},90 H{18 + rx} A{rx},{rx} 0 0 1 18,{90 - rx} "
         f"V{18 + rx} A{rx},{rx} 0 0 1 {18 + rx},18 Z") if rounded else "M0,0 H108 V108 H0 Z"
    return {"d": d, "fill": {"type": "linear", "x1": 18, "y1": 18, "x2": 90, "y2": 90,
                             "stops": [(0, TILE_FROM), (1, TILE_TO)]}}


# ---- SVG

def svg(paths, view):
    defs, body = [], []
    for i, p in enumerate(paths):
        attrs = [f'd="{p["d"]}"']
        fill = p.get("fill")
        if isinstance(fill, dict):
            gid = f"g{i}"
            stops = "".join(f'<stop offset="{o}" stop-color="{c}"/>' for o, c in fill["stops"])
            if fill["type"] == "radial":
                defs.append(f'<radialGradient id="{gid}" cx="{n(fill["cx"])}" cy="{n(fill["cy"])}" '
                            f'r="{n(fill["r"])}" gradientUnits="userSpaceOnUse">{stops}</radialGradient>')
            else:
                defs.append(f'<linearGradient id="{gid}" x1="{fill["x1"]}" y1="{fill["y1"]}" '
                            f'x2="{fill["x2"]}" y2="{fill["y2"]}" gradientUnits="userSpaceOnUse">'
                            f'{stops}</linearGradient>')
            attrs.append(f'fill="url(#{gid})"')
        else:
            attrs.append(f'fill="{fill or "none"}"')
        if "stroke" in p:
            attrs.append(f'stroke="{p["stroke"]}" stroke-width="{n(p["width"])}"')
            if "stroke_alpha" in p:
                attrs.append(f'stroke-opacity="{p["stroke_alpha"]}"')
            if "cap" in p:
                attrs.append(f'stroke-linecap="{p["cap"]}"')
        body.append(f'  <path {" ".join(attrs)}/>')
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="' + view + '" width="1024" height="1024">\n'
            "  <!-- Generated by packaging/icons/make_icons.py -->\n"
            f'  <defs>{"".join(defs)}</defs>\n' + "\n".join(body) + "\n</svg>\n")


# ---- Android vector drawables

def argb(color):
    return "#FF" + color[1:].upper()


def vector(paths):
    """An Android vector drawable of the 108-unit canvas"""
    body = []
    for p in paths:
        attrs = [f'android:pathData="{p["d"]}"']
        inner = ""
        fill = p.get("fill")
        if isinstance(fill, dict):
            items = "".join(f'\n                <item android:offset="{o}" android:color="{argb(c)}"/>'
                            for o, c in fill["stops"])
            if fill["type"] == "radial":
                g = (f'android:type="radial" android:centerX="{n(fill["cx"])}" '
                     f'android:centerY="{n(fill["cy"])}" android:gradientRadius="{n(fill["r"])}"')
            else:
                g = (f'android:type="linear" android:startX="{fill["x1"]}" android:startY="{fill["y1"]}" '
                     f'android:endX="{fill["x2"]}" android:endY="{fill["y2"]}"')
            inner = (f'\n        <aapt:attr name="android:fillColor">\n            <gradient {g}>'
                     f'{items}\n            </gradient>\n        </aapt:attr>\n    ')
        elif fill:
            attrs.append(f'android:fillColor="{argb(fill)}"')
        if "stroke" in p:
            attrs.append(f'android:strokeColor="{argb(p["stroke"])}"')
            attrs.append(f'android:strokeWidth="{n(p["width"])}"')
            if "stroke_alpha" in p:
                attrs.append(f'android:strokeAlpha="{p["stroke_alpha"]}"')
            if "cap" in p:
                attrs.append(f'android:strokeLineCap="{p["cap"]}"')
        sep = "\n        "
        body.append(f"    <path{sep}{sep.join(attrs)}>{inner}</path>" if inner
                    else f"    <path{sep}{sep.join(attrs)}/>")
    return ('<?xml version="1.0" encoding="utf-8"?>\n'
            "<!-- Generated by packaging/icons/make_icons.py -->\n"
            '<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
            '    xmlns:aapt="http://schemas.android.com/aapt"\n'
            '    android:width="108dp" android:height="108dp"\n'
            '    android:viewportWidth="108" android:viewportHeight="108">\n'
            + "\n".join(body) + "\n</vector>\n")


ADAPTIVE = """<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by packaging/icons/make_icons.py -->
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@drawable/ic_launcher_background"/>
    <foreground android:drawable="@drawable/ic_launcher_foreground"/>
    <monochrome android:drawable="@drawable/ic_launcher_monochrome"/>
</adaptive-icon>
"""


def write(rel, text):
    path = os.path.join(ROOT, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)


def main():
    write("packaging/icons/app.svg", svg([tile(True)] + ring(), "18 18 72 72"))
    write(f"{RES}/drawable/ic_launcher_background.xml", vector([tile(False)]))
    write(f"{RES}/drawable/ic_launcher_foreground.xml", vector(ring()))
    write(f"{RES}/drawable/ic_launcher_monochrome.xml", vector(ring(mono=True)))
    write(f"{RES}/mipmap-anydpi/ic_launcher.xml", ADAPTIVE)


if __name__ == "__main__":
    main()
