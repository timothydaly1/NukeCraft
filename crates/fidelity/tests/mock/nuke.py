"""A stand-in for Nuke's `nuke` module, just enough to run the generated oracle script outside
Nuke: it checks the script's logic (graph building, sampling, defaults, the output file), not
Nuke's numbers. Every node class exists, every knob exists, sample() returns 0.25."""

NUKE_VERSION_STRING = 'mock'
env = {'nc': True}
_frame = [1.0]
_live = set()
stats = {'created': 0, 'deleted': 0}


class Knob(object):
    def __init__(self, name):
        self.name = name
        self._values = [0.0]
        self._keys = {}

    def setValue(self, value, index=None):
        if isinstance(value, (list, tuple)):
            self._values = [v for v in value]
        elif index is None:
            self._values = [value]
        else:
            while len(self._values) <= index:
                self._values.append(0.0)
            self._values[index] = value

    def setAnimated(self, index=-1):
        return True

    def setValueAt(self, value, frame, index=0):
        if not isinstance(frame, (int, float)):
            raise TypeError('frame must be a number')
        self._keys[(index, frame)] = value

    def getValueAt(self, frame, index=0):
        if not isinstance(frame, (int, float)):
            raise TypeError('frame must be a number')
        return float(self._keys.get((index, frame), 0.0))

    def value(self, index=0):
        return self._values[index] if index < len(self._values) else 0.0

    def arraySize(self):
        return len(self._values)


class Node(object):
    def __init__(self, cls):
        self._cls = cls
        self._knobs = {}
        self._inputs = {}
        stats['created'] += 1
        _live.add(id(self))

    def Class(self):
        return self._cls

    def knob(self, name):
        if name not in self._knobs:
            self._knobs[name] = Knob(name)
        return self._knobs[name]

    def __getitem__(self, name):
        return self.knob(name)

    def setInput(self, i, node):
        if not isinstance(node, Node):
            raise TypeError('setInput takes a node')
        self._inputs[i] = node
        return True

    def sample(self, channel, x, y, dx=1.0, dy=1.0, frame=None):
        if '.' not in channel:
            raise ValueError('channel must be layer.channel')
        return 0.25


class _Nodes(object):
    def __getattr__(self, cls):
        return lambda **kw: Node(cls)


nodes = _Nodes()
_root = Node('Root')
stats['created'] -= 1


def root():
    return _root


def addFormat(s):
    if len(s.split()) != 4:
        raise ValueError('format string: w h aspect name')
    return s


def frame(f=None):
    if f is not None:
        _frame[0] = f
    return _frame[0]


def delete(node):
    if id(node) not in _live:
        raise ValueError('node deleted twice or never created')
    _live.discard(id(node))
    stats['deleted'] += 1
