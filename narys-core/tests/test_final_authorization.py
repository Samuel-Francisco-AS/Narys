import hashlib,json,os,sqlite3,subprocess,tempfile,unittest
from pathlib import Path
OPS=Path(__file__).resolve().parents[1]/'ops'
PROMPT='Responda somente o número da soma de alpha=2 e beta=3. Não utilize ferramentas, não execute comandos, não acesse arquivos e não faça outras solicitações.'
class Consent(unittest.TestCase):
    def test_durable_scope_never_overwritten_and_receipt_never_reused(self):
        with tempfile.TemporaryDirectory() as home:
            root=Path(home)/'.local/state/narys/core'
            root.mkdir(parents=True,mode=0o700)
            env={'HOME':home,'PATH':'/usr/bin:/bin'}
            def run(script,*args):
                return subprocess.run(['/usr/bin/python3',str(OPS/script),*args],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=3)
            self.assertEqual(run('record_final_consent.py').returncode,0)
            auth=root/'lr10a-final-authorization';original=(auth/'consent.json').read_bytes()
            self.assertNotEqual(run('record_final_consent.py').returncode,0)
            self.assertEqual((auth/'consent.json').read_bytes(),original)
            self.assertEqual((auth/'consent.json').stat().st_mode&0o777,0o600)
            (root/'db').mkdir();db=sqlite3.connect(root/'db/luna.sqlite3')
            db.execute('CREATE TABLE headless_tasks(id INTEGER,objective TEXT,expected TEXT,state TEXT)')
            db.execute('INSERT INTO headless_tasks VALUES (2,?,?,?)',(PROMPT,'5','prepared'));db.commit();db.close()
            self.assertNotEqual(run('review_final_task.py','1').returncode,0)
            self.assertEqual(run('review_final_task.py','2').returncode,0)
            self.assertNotEqual(run('review_final_task.py','2').returncode,0)
            receipt=json.loads((auth/'receipts/task-2.json').read_text())
            self.assertEqual(receipt['objective_sha256'],hashlib.sha256(PROMPT.encode()).hexdigest())
            self.assertFalse(list(auth.glob('attempt-*.json')))
            (auth/'closed.json').write_text('{}')
            self.assertNotEqual(run('review_final_task.py','3').returncode,0)
            self.assertFalse((Path(home)/'.local/state/narys/lr10a-a9-host-attempt.json').exists())
