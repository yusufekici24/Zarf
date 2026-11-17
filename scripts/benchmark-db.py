#!/usr/bin/env python3
"""Sentetik posta kutusu: aynı SQLite liste/sayaç/gruplama sorgularını kıyaslar."""
import sqlite3,time,statistics,json
c=sqlite3.connect(':memory:')
c.executescript('CREATE TABLE messages(id INTEGER PRIMARY KEY,folder_id INTEGER,date REAL,is_pinned INTEGER,is_read INTEGER,tid INTEGER,sender TEXT,subject TEXT); CREATE INDEX idx_messages_list ON messages(folder_id,is_pinned,date,id); CREATE INDEX idx_messages_conv ON messages(folder_id,tid,date,is_read); CREATE INDEX idx_messages_unread ON messages(folder_id,is_read);')
out={}
for n in [10000,100000,300000]:
 c.execute('DELETE FROM messages');c.executemany('INSERT INTO messages VALUES(?,?,?,?,?,?,?,?)',((i,1+i%10,1700000000+i,int(i%997==0),int(i%4!=0),i//5,'Synthetic sender','Synthetic subject') for i in range(n)));c.commit()
 cases={'list_500':('SELECT id,date,sender,subject FROM messages WHERE folder_id=1 ORDER BY is_pinned DESC,date DESC,id DESC LIMIT 500',()),'folder_counts':('SELECT folder_id,COUNT(*),SUM(is_read=0) FROM messages GROUP BY folder_id',()),'conversation_heads':('SELECT id,MAX(date),SUM(is_read=0) FROM messages WHERE folder_id=1 GROUP BY tid',()),'count':('SELECT COUNT(*) FROM messages WHERE folder_id=1',())}
 out[n]={}
 for name,(sql,params) in cases.items():
  times=[]
  for _ in range(11):
   t=time.perf_counter();c.execute(sql,params).fetchall();times.append((time.perf_counter()-t)*1000)
  out[n][name]={'median_ms':round(statistics.median(times[1:]),2),'max_ms':round(max(times[1:]),2),'plan':[r[3] for r in c.execute('EXPLAIN QUERY PLAN '+sql)]}
print(json.dumps(out,indent=2))
