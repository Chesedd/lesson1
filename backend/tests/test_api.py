def test_lesson_order_and_404(client):
    response=client.get('/api/lessons/pandas-intro')
    assert response.status_code==200
    body=response.json();assert [b['order'] for b in body['blocks']]==[1]
    assert [e['order'] for e in body['blocks'][0]['exercises']]==[1,2,3]
    assert client.get('/api/lessons/missing').status_code==404

def test_anonymous_session_is_created_and_reused(client,db_factory):
    first=client.get('/api/lessons/pandas-intro/progress')
    cookie=first.cookies['learner_session']
    second=client.get('/api/lessons/pandas-intro/progress')
    assert second.status_code==200 and client.cookies['learner_session']==cookie
    assert first.headers['set-cookie'].lower().find('httponly')>=0
    from app.models import Learner
    with db_factory() as db: assert db.get(Learner,cookie) is not None

def test_new_learner_progress_and_manifest_total(client):
    assert client.get('/api/lessons/pandas-intro/progress').json()=={'lesson_id':'pandas-intro','completed':0,'total':3,'percent':0,'completed_exercise_ids':[]}
