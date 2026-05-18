import re,os,sys

def extract(path):
    if not os.path.exists(path):
        return None
    s=open(path,'r',encoding='utf-8').read()
    keys=re.findall(r'^\s*"([^"]+)"\s*:', s, flags=re.M)
    vals={}
    for m in re.finditer(r'^\s*"([^"]+)"\s*:\s*"((?:\\"|[^"])*)"\s*,?$', s, flags=re.M):
        vals[m.group(1)] = m.group(2)
    return keys, vals

en='src/lib/locales/en.ts'
es='src/lib/locales/es.ts'
res_en=extract(en)
res_es=extract(es)
if res_en is None or res_es is None:
    print('MISSING_LOCALE_FILES', res_en is None, res_es is None)
    sys.exit(1)

en_keys,en_map=res_en
es_keys,es_map=res_es
set_en=set(en_keys)
set_es=set(es_keys)
missing_in_es=sorted(list(set_en-set_es))
missing_in_en=sorted(list(set_es-set_en))
if missing_in_es:
    print('EN_MISSING_IN_ES')
    for k in missing_in_es: print(k)
if missing_in_en:
    print('ES_MISSING_IN_EN')
    for k in missing_in_en: print(k)
# interpolation parity
mismatch=[]
for k in set_en & set_es:
    a=set(re.findall(r'\{([^}]+)\}', en_map.get(k,'')))
    b=set(re.findall(r'\{([^}]+)\}', es_map.get(k,'')))
    if a!=b:
        mismatch.append((k,sorted(a),sorted(b)))
if mismatch:
    print('PLACEHOLDER_MISMATCH')
    for k,a,b in mismatch:
        print(k, a, b)
if missing_in_es or missing_in_en or mismatch:
    sys.exit(2)
print('LOCALE_OK')
sys.exit(0)
