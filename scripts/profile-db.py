#!/usr/bin/env python3
"""Salt okunur SQLite tanısı. İleti içerikleri, adresler ve parolalar yazdırılmaz."""
import argparse, json, sqlite3, statistics, time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('database', type=Path);p.add_argument('--fts-term', help='İsteğe bağlı FTS MATCH ifadesi; rapora yazılmaz');a=p.parse_args()
c=sqlite3.connect(a.database.resolve().as_uri()+'?mode=ro',uri=True,timeout=5)
c.execute('PRAGMA query_only=ON')
out={'sqlite':sqlite3.sqlite_version,'file_bytes':a.database.stat().st_size,'wal_bytes':Path(str(a.database)+'-wal').stat().st_size if Path(str(a.database)+'-wal').exists() else 0}
for pragma in ['page_size','page_count','freelist_count','journal_mode']:out[pragma]=c.execute('PRAGMA '+pragma).fetchone()[0]
out['counts']={t:c.execute('SELECT COUNT(*) FROM '+t).fetchone()[0] for t in ['messages','message_content','contacts','folders']}
f=c.execute('SELECT folder_id FROM messages GROUP BY folder_id ORDER BY COUNT(*) DESC LIMIT 1').fetchone()
out['queries']={}
if f:
 for name,sql,params in [
 ('list','SELECT id,date,sender,subject FROM messages WHERE folder_id=? ORDER BY is_pinned DESC,date DESC,id DESC LIMIT 500',f),
 ('count','SELECT COUNT(*) FROM messages WHERE folder_id=?',f),
 ('folder_counts','SELECT folder_id,COUNT(*),SUM(is_read=0) FROM messages GROUP BY folder_id',()),
 ('conversation_heads','SELECT id,MAX(date),SUM(is_read=0) FROM messages WHERE folder_id=? GROUP BY tid',f)]:
  plan=[r[3] for r in c.execute('EXPLAIN QUERY PLAN '+sql,params)];times=[]
  for _ in range(11):
   t=time.perf_counter();c.execute(sql,params).fetchall();times.append((time.perf_counter()-t)*1000)
  out['queries'][name]={'plan':plan,'first_ms':round(times[0],3),'warm_median_ms':round(statistics.median(times[1:]),3),'warm_max_ms':round(max(times[1:]),3)}
try:
 out['fts_build']=[{'position':r[0],'last_id':r[1]} for r in c.execute('SELECT pos,last FROM fts_build')]
 out['fts_version']=c.execute("SELECT value FROM settings WHERE key='fts_version'").fetchone()
 if a.fts_term:
  t=time.perf_counter();n=c.execute('SELECT COUNT(*) FROM message_fts WHERE message_fts MATCH ?', (a.fts_term,)).fetchone()[0]
  out['fts_search']={'matches':n,'elapsed_ms':round((time.perf_counter()-t)*1000,3)}
except sqlite3.Error:
 out['fts_status']='FTS metadata veya sorgusu bu SQLite sürümüyle okunamadı; uygulamanın gömülü SQLite sürümü farklı olabilir.'
print(json.dumps(out,ensure_ascii=False,indent=2))
