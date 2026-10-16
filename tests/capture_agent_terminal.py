"""Run Reader's production event loop in Windows ConPTY with isolated fixtures.

PNG files are rasterizations of the captured VT cell buffer, not design mockups
and not screenshots of the Windows Terminal GUI/font renderer.
"""
import sys, os, time, select, json
from pathlib import Path

ROOT = Path(os.environ.get('READER_DIR', Path(__file__).resolve().parent.parent))
if not (ROOT / 'Cargo.toml').exists():
    ROOT = Path.cwd()

test_tools = os.environ.get('TEST_TOOLS_DIR')
if test_tools:
    sys.path.insert(0, test_tools)
else:
    for candidate in [ROOT.parent / 'test-tools', Path('G:/Document/reader/test-tools')]:
        if candidate.exists():
            sys.path.insert(0, str(candidate))
            break

from winpty import PtyProcess
import pyte
from PIL import Image, ImageDraw, ImageFont

OUT = ROOT / 'doc/agent-terminal-design/runtime'
OUT.mkdir(exist_ok=True)

def find_test_exe(root: Path) -> Path:
    explicit = os.environ.get('READER_TEST_EXE')
    if explicit and Path(explicit).exists():
        return Path(explicit)
    deps = root / 'target/debug/deps'
    candidates = sorted(deps.glob('reader-*.exe'), key=lambda p: p.stat().st_mtime, reverse=True)
    import subprocess
    for c in candidates:
        try:
            out = subprocess.run([str(c), '--list'], capture_output=True, text=True, timeout=5).stdout
            if 'tui::tests::terminal_fixture' in out:
                return c
        except Exception:
            continue
    if candidates:
        return candidates[0]
    return root / 'target/debug/deps/reader-ae9ae9b1880c8aca.exe'

EXE = find_test_exe(ROOT)
SAMPLE = (ROOT / 'doc/agent-terminal-design/sample.txt').read_text(encoding='utf-8')
FONT = ImageFont.truetype('C:/Windows/Fonts/consola.ttf', 20)
BOLD = ImageFont.truetype('C:/Windows/Fonts/consolab.ttf', 20)
CJK = ImageFont.truetype('C:/Windows/Fonts/msyh.ttc', 20)
SYM = ImageFont.truetype('C:/Windows/Fonts/seguisym.ttf', 20)
PALETTE = {
    'default': '#e2e3e5', 'black': '#17191c', 'red': '#c45050', 'green': '#6fa878',
    'brown': '#b0a260', 'blue': '#6489bb', 'magenta': '#a977af', 'cyan': '#83bbc4',
    'white': '#d3d3d3', 'brightblack': '#888b91', 'brightred': '#ed7777',
    'brightgreen': '#9aca9a', 'brightyellow': '#e7d58a', 'brightblue': '#94b5e8',
    'brightmagenta': '#d0a2dd', 'brightcyan': '#a1d5dd', 'brightwhite': '#ffffff'
}

def raster(screen, path):
    im = Image.new('RGB', (screen.columns * 12, screen.lines * 28), '#17191c')
    d = ImageDraw.Draw(im)
    for y in range(screen.lines):
        for x in range(screen.columns):
            cell = screen.buffer[y][x]
            c = cell.data
            if not c or c == ' ':
                continue
            f = CJK if ord(c[0]) > 0x2e80 else (BOLD if cell.bold else FONT)
            if c in '❯✓●▸└':
                f = SYM
            color = PALETTE.get(cell.fg, '#' + cell.fg if len(cell.fg) == 6 else '#e2e3e5')
            if c == '─':
                d.line((x * 12, y * 28 + 14, x * 12 + 12, y * 28 + 14), fill=color)
            else:
                d.text((x * 12, y * 28 + 2), c, font=f, fill=color)
    im.save(path)

class VtScreen(pyte.Screen):
    # pyte does not implement xterm's alternate screen. ConPTY emits 1049h
    # when Ratatui enters it; initialize that distinct, blank cell buffer.
    def set_mode(self, *modes, **kwargs):
        if kwargs.get('private') and 1049 in modes:
            self.reset()
        super().set_mode(*modes, **kwargs)

class Session:
    def __init__(self, w, h):
        self.w, self.h = w, h
        self.dir = ROOT / 'target/terminal-fixtures' / f'{w}x{h}-{time.time_ns()}'
        self.dir.mkdir(parents=True)
        (self.dir / 'SECRET_TITLE - SECRET_AUTHOR.txt').write_text(SAMPLE * 8, encoding='utf-8')
        env = dict(os.environ, READER_TERMINAL_FIXTURE=str(self.dir), TERM='xterm-256color')
        env.pop('NO_COLOR', None)
        self.proc = PtyProcess.spawn(
            [str(EXE), '--exact', 'tui::tests::terminal_fixture', '--ignored', '--nocapture', '--test-threads=1'],
            cwd=str(ROOT),
            env=env,
            dimensions=(h, w),
            backend=0
        )
        self.screen = VtScreen(w, h)
        self.stream = pyte.Stream(self.screen)
        self.raw = ''

    def pump(self, seconds=0.45):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if select.select([self.proc.fileobj], [], [], 0.03)[0]:
                try:
                    chunk = self.proc.read(65536)
                except EOFError:
                    break
                self.raw += chunk
                self.stream.feed(chunk)
                if '\x1b[6n' in chunk:
                    self.proc.write('\x1b[1;1R')
                if '\x1b[c' in chunk:
                    self.proc.write('\x1b[?1;2c')

    def text(self):
        return '\n'.join(self.screen.display)

    def key(self, key):
        self.proc.write(key)
        self.pump()

    def resize(self, w, h):
        self.w, self.h = w, h
        self.screen.resize(h, w)
        self.proc.setwinsize(h, w)
        self.pump(0.65)

    def capture(self, name):
        text = self.text()
        (OUT / f'{name}.txt').write_text(text + '\n', encoding='utf-8')
        raster(self.screen, OUT / f'{name}.png')
        return text

    def position(self):
        return json.loads((self.dir / 'library.json').read_text(encoding='utf-8'))['books'][0]['progress']['position']

    def close(self):
        if self.proc.isalive():
            self.key('\x03')
        self.pump(0.4)
        for _ in range(50):
            if not self.proc.isalive():
                break
            self.pump(0.1)
        status = self.proc.exitstatus
        (OUT / f'{self.original_size}.ansi').write_text(self.raw, encoding='utf-8')
        self.proc.close(force=True)
        assert status == 0, (status, self.text())

def run_tests():
    results = []
    for w, h in [(80, 24), (120, 30), (60, 18)]:
        s = Session(w, h)
        s.original_size = f'{w}x{h}'
        try:
            for _ in range(20):
                s.pump(0.25)
                if 'read_file path="corpus/zh_sample.txt"' in s.text():
                    break
            normal = s.capture(f'{w}x{h}-reading')
            assert 'read_file path="corpus/zh_sample.txt"' in normal, normal
            assert 'corpus/zh_sample.txt' in normal, normal
            assert '雨停以后' in normal, normal
            assert 'SECRET_' not in normal, normal
            assert 'j/k scroll' not in normal, normal
            hh = 3 if w < 72 or h < 20 else 4
            before = list(s.screen.display)[hh:h - 4]
            s.key('j')
            scrolled = s.capture(f'{w}x{h}-scrolled')
            after = list(s.screen.display)[hh:h - 4]
            assert before[2:] == after[:-2], (before, after)
            position = s.position()
            s.key('b')
            s.pump(0.5)
            checkpoint = s.capture(f'{w}x{h}-checkpoint')
            assert 'Checkpoint saved' in checkpoint, checkpoint
            assert after == list(s.screen.display)[hh:h - 4]
            bookmarks = json.loads((s.dir / 'library.json').read_text(encoding='utf-8'))['books'][0]['bookmarks']
            assert bookmarks[-1]['position'] == position
            s.key('\x1b[24~')
            folded = s.capture(f'{w}x{h}-folded')
            for forbidden in ['SECRET_', '雨停', '桥头', 'Checkpoint saved', '第', 'G:\\']:
                assert forbidden not in folded, (forbidden, folded)
            assert 'Output folded' in folded, folded
            assert s.position() == position
            for nw, nh in [(60, 18), (120, 30), (80, 24)]:
                s.resize(nw, nh)
                assert '雨停' not in s.text() and 'SECRET_' not in s.text()
            s.resize(w, h)
            s.key('\x1b[24~')
            assert list(s.screen.display)[hh:h - 4] == after
            assert s.position() == position
            # Expanded reflow must preserve the exact stored character position too.
            for nw, nh in [(60, 18), (120, 30), (80, 24), (w, h)]:
                s.resize(nw, nh)
                assert s.position() == position
            s.key('p')
            assert 'SECRET_TITLE' in s.text()
            s.key('p')
            assert 'SECRET_' not in s.text()
            assert s.position() == position
            s.key(':down\r')
            assert s.position() != position
            s.key(':private_query')
            assert 'private_query' in s.text()
            s.key('\x1b[24~')
            assert 'private_query' not in s.text()
            s.key('\x1b[24~')
            assert 'private_query' not in s.text()
            # Ordinary search overlay -> fold -> resize -> restore cannot resurrect it.
            s.key('p')
            s.key('/SECRET_QUERY')
            assert 'SECRET_QUERY' in s.text()
            s.key('\x1b[24~')
            assert 'SECRET_' not in s.text()
            s.resize(60, 18)
            assert 'SECRET_' not in s.text()
            s.key('\x1b[24~')
            assert 'SECRET_QUERY' not in s.text()
            s.key('p')
            assert 'SECRET_' not in s.text()
            idle = s.text()
            s.pump(0.7)
            assert s.text() == idle
            results.append({
                'size': f'{w}x{h}',
                'backend': 'Windows ConPTY',
                'scroll': 'pass',
                'checkpoint': 'pass',
                'fold_and_resize': 'pass',
                'exact_position': position,
                'normal_mode_switch': 'pass',
                'commands': 'pass',
                'search_draft_redaction': 'pass',
                'idle_frame': 'pass'
            })
            print(f'{w}x{h} ConPTY passed', flush=True)
        finally:
            s.close()
    (OUT / 'results.json').write_text(json.dumps(results, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print('All ConPTY scenarios passed', flush=True)

if __name__ == '__main__':
    run_tests()
