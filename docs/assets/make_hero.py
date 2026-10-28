import math, cairosvg
W,H=1600,480
ACC="#4F6BFF"   # one soft accent (lighter cousin of the repo's #1234FF)
STACK="-apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif"
# waveform: decaying burst flowing into a text cursor + placeholder text lines
x0,x1,cy=930,1330,240
pts=[]
for i in range(0,401):
    x=x0+i*(x1-x0)/400; t=i/400
    env=math.sin(math.pi*min(t/0.85,1))**1.6*(1-t)**0.6
    y=cy-92*env*math.sin(2*math.pi*t*11)
    pts.append(f"{x:.1f},{y:.1f}")
wave="M"+" L".join(pts)
def svg(stack):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" role="img" aria-labelledby="t d">
  <title id="t">Coco Voice</title>
  <desc id="d">Coco Voice. Hold a key, speak, and your words land in the app you already have open. The speech model runs on your own computer.</desc>
  <rect width="{W}" height="{H}" fill="#FAFAF8"/>
  <text x="112" y="214" font-family="{stack}" font-size="92" font-weight="700" fill="#0B0B0B" letter-spacing="-1.5">Coco Voice</text>
  <text x="114" y="284" font-family="{stack}" font-size="31" fill="#3C3C37">Hold a key, speak, and your words land</text>
  <text x="114" y="326" font-family="{stack}" font-size="31" fill="#3C3C37">in the app you already have open.</text>
  <text x="114" y="384" font-family="{stack}" font-size="22" fill="#5F5F58">The speech model runs on your own computer.</text>
  <line x1="860" y1="{cy}" x2="1500" y2="{cy}" stroke="#E3E3DE" stroke-width="2"/>
  <g transform="translate(872,{cy-30})" fill="none" stroke="#3C3C37" stroke-width="3.5" stroke-linecap="round">
    <rect x="0" y="0" width="26" height="40" rx="13"/>
    <path d="M-8 26 a21 21 0 0 0 42 0 M13 47 v14 M2 61 h22"/>
  </g>
  <path d="{wave}" fill="none" stroke="{ACC}" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>
  <line x1="1346" y1="{cy-34}" x2="1346" y2="{cy+34}" stroke="{ACC}" stroke-width="4" stroke-linecap="round"/>
  <g fill="#D6D6D0">
    <rect x="1366" y="{cy-16}" width="58" height="10" rx="5"/><rect x="1432" y="{cy-16}" width="44" height="10" rx="5"/>
    <rect x="1366" y="{cy+6}" width="40" height="10" rx="5"/><rect x="1414" y="{cy+6}" width="62" height="10" rx="5"/>
  </g>
</svg>
'''
open("coco-voice-hero.svg","w").write(svg(STACK))
# The PNG renders with DejaVu Sans, a system font (no web fonts)
cairosvg.svg2png(bytestring=svg("DejaVu Sans").encode(), write_to="coco-voice-hero.png", output_width=W, output_height=H)
