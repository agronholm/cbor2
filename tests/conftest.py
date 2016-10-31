from collections import namedtuple
from operator import attrgetter

Contender = namedtuple('Contender', 'name,dumps,loads')

contenders = []

import json
contenders.append(Contender('json', json.dumps, json.loads))

import cbor2
contenders.append(Contender('cbor2', cbor2.dumps, cbor2.loads))


# See https://github.com/ionelmc/pytest-benchmark/issues/48

def pytest_benchmark_group_stats(config, benchmarks, group_by):
    result = {}
    for bench in benchmarks:
        engine, data_kind = bench.param.split('-')
        group = result.setdefault("%s: %s" % (data_kind, bench.group), [])
        group.append(bench)
    return sorted(result.items())

def pytest_generate_tests(metafunc):
    if 'contender' in metafunc.fixturenames:
        metafunc.parametrize('contender', contenders, ids=attrgetter('name'))
