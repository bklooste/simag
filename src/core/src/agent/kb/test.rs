use super::*;

use std::collections::HashSet;

#[test]
fn repr_inference_ask_pred() {
    let test_01 = String::from("
        ( professor[$Lucy,u=1] )
    ");
    let q01_01 = "(professor[$Lucy,u=1] && person[$Lucy,u=1])".to_string();
    let q01_02 = "(professor[$Lucy,u=1])".to_string();
    let mut rep = Representation::new();
    rep.tell(test_01).unwrap();
    assert_eq!(rep.ask(q01_01).unwrap().get_results_single(), None);
    assert_eq!(rep.ask(q01_02).unwrap().get_results_single(), Some(true));

    let test_02 = String::from("
        ( professor[$Lucy,u=1] )
        ( dean[$John,u=1] )
        ( ( let x ) ( dean[x,u=1] := professor[x,u=1] ) )
        ( ( let x ) ( professor[x,u=1] := person[x,u=1] ) )
    ");
    let q02_01 = "(professor[$Lucy,u>0] && person[$Lucy,u<1])".to_string();
    let q02_02 = "(person[$John,u=1])".to_string();
    let mut rep = Representation::new();
    rep.tell(test_02).unwrap();
    assert_eq!(rep.ask(q02_01).unwrap().get_results_single(), Some(false));
    assert_eq!(rep.ask(q02_02).unwrap().get_results_single(), Some(true));

    let test_03 = String::from("
        ( fn::owns[$M1,u=1;$Nono] )
        ( missile[$M1,u=1] )
        ( american[$West,u=1] )
        ( fn::enemy[$Nono,u=1;$America] )
        (( let x, y, z )
            (( american[x,u=1] && weapon[y,u=1] && fn::sells[y,u=1;x;z] && hostile[z,u=1]  )
                := criminal[x,u=1] ))
        (( let x )
            (( fn::owns[x,u=1;$Nono] && missile[x,u=1] ) := fn::sells[x,u=1;$West;$Nono] ))
        (( let x ) ( missile[x,u=1] := weapon[x,u=1] ) )
        (( let x ) ( fn::enemy[x,u=1;$America] := hostile[x,u=1] ) )
    ");
    let q03_01 = "(criminal[$West,u=1]) && hostile[$Nono,u=1] && weapon[$M1,u=1]".to_string();
    let mut rep = Representation::new();
    rep.tell(test_03).unwrap();
    let answ = rep.ask(q03_01);
    assert_eq!(answ.unwrap().get_results_single(), Some(true));

    let test_04 = String::from("
        # query for all 'professor'
        ( professor[$Lucy,u=1] )
        ( dean[$John,u=1] )
        ((let x) (dean[x,u=1] := professor[x,u=1]))
    ");
    let q04_01 = "((let x) (professor[x,u=1]))".to_string();
    let mut rep = Representation::new();
    rep.tell(test_04).unwrap();
    let answ = rep.ask(q04_01);
    let a04_01 = answ.unwrap().get_memberships();
    assert!(a04_01.contains_key("$Lucy"));
    assert!(a04_01.contains_key("$John"));

    let test_05 = String::from("
        # query for all classes '$Lucy' is member of
        (professor[$Lucy,u=1])
        ((let x) (professor[x,u=1] := person[x,u=1]))
        (ugly[$Lucy,u=0.2])
    ");
    let q05_01 = "((let x) (x[$Lucy,u>0.5]))".to_string();
    let mut rep = Representation::new();
    rep.tell(test_05).unwrap();
    let mut results = HashSet::new();
    results.insert("professor");
    results.insert("person");
    let answ = rep.ask(q05_01);
    let a05_01 = answ.unwrap().get_memberships();
    let mut cmp = HashSet::new();
    for a in a05_01.get("$Lucy").unwrap() {
        cmp.insert(a.get_parent());
    }
    assert_eq!(results, cmp);
}

#[test]
fn repr_inference_ask_func() {
    let test_01 = String::from("
        ( professor[$Lucy,u=1] )
        ( dean[$John,u=1] )
        ( fn::criticize[$John,u=1;$Lucy] )
    ");
    let q01_01 = "(fn::criticize[$John,u=1;$Lucy])".to_string();
    let mut rep = Representation::new();
    rep.tell(test_01).unwrap();
    assert_eq!(rep.ask(q01_01).unwrap().get_results_single(), Some(true));

    let test_02 = String::from("
        ( animal[cow,u=1] )
        ( female[cow,u=1] )
        ( (let x) (animal[x,u=1] && female[x,u=1]) := fn::produce[milk,u=1;x] )
    ");
    let q02_01 = "(fn::produce[milk,u=1;cow])".to_string();
    let mut rep = Representation::new();
    rep.tell(test_02).unwrap();
    assert_eq!(rep.ask(q02_01).unwrap().get_results_single(), Some(true));

    let test_03 = String::from("
        ( professor[$Lucy,u=1] )
        ( dean[$John,u=1] )
        ( fn::criticize[$John,u=1;$Lucy] )
        ( (let x) ( dean[x,u=1] := professor[x,u=1] ) )
        ( (let x) ( professor[x,u=1] := person[x,u=1] ) )
        ( (let x, y)
            (( person[x,u=1] && person[y,u=1] && dean[y,u=1] && fn::criticize[y,u=1;x] )
                := fn::friend[x,u=0;y] ))
    ");
    let q03_01 = "(fn::friend[$Lucy,u=0;$John])".to_string();
    let mut rep = Representation::new();
    rep.tell(test_03).unwrap();
    assert_eq!(rep.ask(q03_01).unwrap().get_results_single(), Some(true));

    let test_04 = String::from("
        # retrieve all objs which fit to a criteria
        (fn::produce[milk,u=1;$Lulu])
        (cow[$Lucy,u=1])
        (goat[$Vicky,u=1])
        ((let x) ((cow[x,u=1] || goat[x,u=1]) := (female[x,u=1] && animal[x,u=1])))
        ((let x) ((female[x,u>0] && animal[x,u>0]) := fn::produce[milk,u=1;x]))
    ");
    let q04_01 = "((let x) (fn::produce[milk,u>0;x]))".to_string();
    let mut rep = Representation::new();
    rep.tell(test_04).unwrap();
    let answ = rep.ask(q04_01);
    let a04_01 = answ.unwrap().get_relationships();
    assert!(a04_01.contains_key("$Lucy"));
    assert!(a04_01.contains_key("$Lulu"));
    assert!(a04_01.contains_key("$Vicky"));

    let test_05 = String::from("
        # retrieve all relations between objects
        (fn::loves[$Vicky,u=1;$Lucy])
        (fn::worships[$Vicky,u=1;cats])
        (fn::hates[$Vicky,u=0;dogs])
    ");
    let q05_01 = "((let x) (fn::x[$Vicky,u>0;$Lucy]))".to_string();
    let mut rep = Representation::new();
    rep.tell(test_05).unwrap();
    let mut results = HashSet::new();
    results.insert("loves");
    results.insert("worships");
    let answ = rep.ask(q05_01);
    let a05_01 = answ.unwrap().get_relationships();
    let mut cnt = 0;
    for a in a05_01.get("$Vicky").unwrap() {
        cnt += 1;
        assert!(results.contains(a.get_name()));
        assert!(a.get_name() != "hates");
    }
    assert_eq!(cnt, 2);

    let q05_02 = "((let x, y) (fn::x[$Vicky,u=0;y]))".to_string();
    let answ = rep.ask(q05_02);
    let a05_02 = answ.unwrap().get_relationships();
    let mut cnt = 0;
    for a in a05_02.get("$Vicky").unwrap() {
        cnt += 1;
        assert!(a.get_name() == "hates");
    }
    assert_eq!(cnt, 1);
}

#[test]
fn repr_inference_time_calc() {
    let test_01 = String::from("
        (( let x, y, t1:time, t2:time=\"Now\" )
            (( dog[x,u=1] && meat[y,u=1] && fn::eat(t1=time)[y,u=1;x] && fn::time_calc(t1<t2) )
            := fat(time=t2)[x,u=1] ))
        ( dog[$Pancho,u=1] )
        ( meat[$M1,u=1] )
        ( fn::eat(time=\"2014-07-05T10:25:00Z\")[$M1,u=1;$Pancho] )
    ");
    let q01_01 = "(fat(time='Now')[$Pancho,u=1])".to_string();
    let mut rep = Representation::new();
    rep.tell(test_01).unwrap();
    assert_eq!(rep.ask(q01_01).unwrap().get_results_single(), Some(true));

    let test_02 = String::from("
        (( let x, y, t1: time=\"2015-07-05T10:25:00Z\", t2: time )
            ( ( dog[x,u=1] && meat[y,u=1] && fat(t2=time)[x,u=1] && fn::time_calc(t2<t1) )
            := fn::eat(time=t1)[y,u=1;x]
            )
        )
        ( dog[$Pancho,u=1] )
        ( meat[$M1,u=1] )
        ( fat(time=\"2014-07-05T10:25:00Z\")[$Pancho,u=1] )
    ");
    let q02_01 = "(fn::eat(time='Now')[$M1,u=1;$Pancho])".to_string();
    let mut rep = Representation::new();
    rep.tell(test_02).unwrap();
    assert_eq!(rep.ask(q02_01).unwrap().get_results_single(), Some(true));

    // Test 03
    let mut rep = Representation::new();
    let test_03_00 = String::from("
        (meat[$M1,u=1])
        (dog[$Pancho,u=1])
    ");
    rep.tell(test_03_00).unwrap();

    let test_03_01 = String::from("
        (fn::eat(time='2015-01-01T00:00:00Z')[$M1,u=1;$Pancho])
        ((let x, y)
            ((dog[x,u=1] && meat[y,u=1] && fn::eat[y,u=1;x])
            := fat[x,u=1]))
    ");
    rep.tell(test_03_01).unwrap();
    let q03_01 = "(fat[$Pancho,u=1])".to_string();
    assert_eq!(rep.ask(q03_01).unwrap().get_results_single(), Some(true));

    let test_03_02 = String::from("
        (run(time='2015-01-01T00:00:00Z')[$Pancho,u=1])
        ((let x) (( dog[x,u=1] && run[x,u=1] ) := fat[x,u=0]))
    ");
    rep.tell(test_03_02).unwrap();
    let q03_02 = "(fat[$Pancho,u=0])".to_string();
    assert_eq!(rep.ask(q03_02).unwrap().get_results_single(), Some(true));

    let test_03_03 = String::from("
        (run(time='2015-01-01T00:00:00Z')[$Pancho,u=1])
        (fn::eat(time='2015-02-01T00:00:00Z')[$M1,u=1;$Pancho])
        ((let x, y, t1:time, t2:time)
            (run(t1=time)[x,u=1] && fn::eat(t2=time)[y,u=1;x]
            && dog[x,u=1] && meat[y,u=1] && fn::time_calc(t1<t2))
            := (fat[x,u=1] || fat[x,u=0]))
    ");
    rep.tell(test_03_03).unwrap();
    let q03_03 = "(fat[$Pancho,u=1])".to_string();
    assert_eq!(rep.ask(q03_03).unwrap().get_results_single(), Some(true));

    let test_03_04 = String::from("
        (fn::eat(time='2015-01-02T00:00:00Z', overwrite)[$M1,u=1;$Pancho])
        (run(time='2015-02-01T00:00:00Z', overwrite)[$Pancho,u=1])
    ");
    rep.tell(test_03_04).unwrap();
    let q03_04 = "(fat[$Pancho,u=0])".to_string();
    assert_eq!(rep.ask(q03_04).unwrap().get_results_single(), Some(true));
}


#[test]
#[allow(unused_must_use)]
fn repr_eval_fol() {
    // indicative conditional
    // (true := true)
    let mut rep = Representation::new();
    let fol = String::from("
            ( drugDealer[$West,u=1] := ( scum[$West,u=1] && good[$West,u=0] ) )
            ( drugDealer[$West,u=1] )
        ");
    let query = "( scum[$West,u=1] && good[$West,u=0] )".to_string();
    rep.tell(fol);
    assert_eq!(rep.ask(query).unwrap().get_results_single(), Some(true));

    // (false := none)
    let mut rep = Representation::new();
    let fol = String::from("
            ( drugDealer[$West,u=1] := ( scum[$West,u=1] && good[$West,u=0] ) )
            ( drugDealer[$West,u=0] )
        ");
    let query = "( scum[$West,u=1] && good[$West,u=0] )".to_string();
    rep.tell(fol);
    assert_eq!(rep.ask(query).unwrap().get_results_single(), None);

    // material implication statements
    // true (true => true)
    let mut rep = Representation::new();
    let fol = String::from("
            ( drugDealer[$West,u=1] => ( scum[$West,u=1] && good[$West,u=0] ) )
            ( drugDealer[$West,u=1] && scum[$West,u=1] && good[$West,u=0] )
        ");
    let query = "( drugDealer[$West,u=1] && scum[$West,u=1] && good[$West,u=0] )".to_string();
    rep.tell(fol);
    assert_eq!(rep.ask(query).unwrap().get_results_single(), Some(true));

    // true (false => true)
    let mut rep = Representation::new();
    let fol = String::from("
            ( drugDealer[$West,u=1] => ( scum[$West,u=1] && good[$West,u=0] ) )
            ( drugDealer[$West,u=0] && scum[$West,u=1] && good[$West,u=0] )
        ");
    let query = "( drugDealer[$West,u=0] && scum[$West,u=1] && good[$West,u=0] )".to_string();
    rep.tell(fol);
    assert_eq!(rep.ask(query).unwrap().get_results_single(), Some(true));

    // false (true => false)
    let mut rep = Representation::new();
    let fol = String::from("
            ( drugDealer[$West,u=1] => ( scum[$West,u=1] && good[$West,u=0] ) )
            ( drugDealer[$West,u=1] && scum[$West,u=0] && good[$West,u=1] )
        ");
    let query0 = "( drugDealer[$West,u=1] )".to_string();
    let query1 = "( scum[$West,u=0] && good[$West,u=1] )".to_string();
    rep.tell(fol);
    assert_eq!(rep.ask(query0).unwrap().get_results_single(), Some(true));
    assert_eq!(rep.ask(query1).unwrap().get_results_single(), None);

    // true (false => false)
    let mut rep = Representation::new();
    let fol = String::from("
            ( drugDealer[$West,u=1] => ( scum[$West,u=1] && good[$West,u=0] ) )
            ( drugDealer[$West,u=0] && scum[$West,u=0] && good[$West,u=1] )
        ");
    let query0 = "( drugDealer[$West,u=0] )".to_string();
    let query1 = "( scum[$West,u=0] && good[$West,u=1] )".to_string();
    rep.tell(fol);
    assert_eq!(rep.ask(query0).unwrap().get_results_single(), Some(true));
    assert_eq!(rep.ask(query1).unwrap().get_results_single(), Some(true));

    // equivalence statements
    // is false (false <=> true )
    let mut rep = Representation::new();
    let fol = String::from("
            ( drugDealer[$West,u=1] <=> ( scum[$West,u=1] && good[$West,u=0] ) )
            ( scum[$West,u=1] )
            ( good[$West,u=0] )
            ( drugDealer[$West,u=0] )
        ");
    let query = "( drugDealer[$West,u=0] )".to_string();
    rep.tell(fol);
    assert_eq!(rep.ask(query).unwrap().get_results_single(), None);

    // is false (true <=> false )
    let mut rep = Representation::new();
    let fol = String::from("
            ( drugDealer[$West,u=1] <=> ( scum[$West,u=1] && good[$West,u=0] ) )
            ( drugDealer[$West,u=1] )
            ( scum[$West,u=0] )
            ( good[$West,u=1] )
        ");
    let query = "( scum[$West,u=1] && good[$West,u=0] )".to_string();
    rep.tell(fol);
    assert_eq!(rep.ask(query).unwrap().get_results_single(), None);

    // is true ( true <=> true )
    let mut rep = Representation::new();
    let fol = String::from("
            ( drugDealer[$West,u=1] <=> ( scum[$West,u=1] && good[$West,u=0] ) )
            ( scum[$West,u=1] )
            ( good[$West,u=0] )
            ( drugDealer[$West,u=1] )
        ");
    let query = "( drugDealer[$West,u=1] && scum[$West,u=1] && good[$West,u=0] )".to_string();
    rep.tell(fol);
    assert_eq!(rep.ask(query).unwrap().get_results_single(), Some(true));

    // is true ( false <=> false )
    let mut rep = Representation::new();
    let fol = String::from("
            ( drugDealer[$West,u=1] <=> ( scum[$West,u=1] && good[$West,u=0] ) )
            ( scum[$West,u=0] )
            ( good[$West,u=1] )
            ( drugDealer[$West,u=0] )
        ");
    let query = "( drugDealer[$West,u=0] && scum[$West,u=0] && good[$West,u=1] )".to_string();
    rep.tell(fol);
    assert_eq!(rep.ask(query).unwrap().get_results_single(), Some(true));
}

