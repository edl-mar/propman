// =============================================================================
// COLUMNS — domänenunabhängig, lebt bewusst AUSSERHALB von `app` (genau wie
// ganz am Anfang begründet: kennt weder User noch Post, wäre in einem
// komplett anderen Projekt wiederverwendbar).
//
// WICHTIGER UNTERSCHIED zur allerersten Version: `Idx` hat NUR `idx()`
// (ID → Zahl), NICHT `from_idx()` (Zahl → ID). Hätte `Dense` selbst die
// Fähigkeit, aus einer beliebigen Zahl eine `UserId` zu konstruieren,
// bräuchte `from_idx` einen `pub`-Trait-Methoden-Zugriff — und JEDER mit
// `use columns::Idx;` im Scope könnte sich damit eine `UserId` erfinden,
// egal ob gültig oder nicht. Das würde genau das Loch wieder aufreißen,
// das wir uns mit dem privaten Tupel-Feld zugemacht hatten. Stattdessen
// gibt `Dense::push` nur die rohe Position (`usize`) zurück — die
// eigentliche `UserId`-Konstruktion bleibt exklusiv bei `UserTable`
// selbst, die als Einzige das private Tupel-Feld anfassen darf.
// =============================================================================
 
mod columns {
    pub trait Idx: Copy {
        fn idx(self) -> usize;
    }
 
    /// Macht die Beziehung "diese Zeilen gehören zu dieser ID-Art" im
    /// Typsystem sichtbar — anders als bei einem nackten `Vec<T>`, das
    /// nichts darüber aussagt, wer damit indizieren darf.
    pub struct Dense<Id, T> {
        data: Vec<T>,
        _marker: std::marker::PhantomData<Id>,
    }
 
    impl<Id: Idx, T> Dense<Id, T> {
        pub fn new() -> Self {
            Self {
                data: Vec::new(),
                _marker: std::marker::PhantomData,
            }
        }
 
        /// Liefert nur die rohe Position zurück, KEINE `Id` — die
        /// Konstruktion der eigentlichen ID bleibt Sache des Aufrufers
        /// (der dafür in seinem eigenen Modul sein muss, wo das
        /// ID-Tupel-Feld privat zugänglich ist).
        pub fn push(&mut self, value: T) -> usize {
            let idx = self.data.len();
            self.data.push(value);
            idx
        }
 
        pub fn get(&self, id: Id) -> &T {
            &self.data[id.idx()]
        }
        pub fn get_mut(&mut self, id: Id) -> &mut T {
            &mut self.data[id.idx()]
        }
 
        /// Gibt `(usize, &T)`-Paare zurück, keine `Id` — aus demselben
        /// Grund wie bei `push`. Der Aufrufer (im richtigen Modul) baut
        /// sich daraus seine eigene `Id`.
        pub fn iter(&self) -> impl Iterator<Item = (usize, &T)> + '_ {
            self.data.iter().enumerate()
        }
    }
}
 
mod app {
 
    // ── `detail` (= Store) ist der ELTERNTEIL von `user_table` und
    // `post_table`, nicht ihr Geschwister. Dadurch zeigt `pub(super)`
    // auf `UserWriter`/`PostWriter` exakt auf `detail`, statt auf
    // ganz `app` — die Sichtbarkeitsgrenze folgt der tatsächlich
    // gewollten Zugriffsgrenze. `detail` ist der aus C++/Boost bekannte
    // Name für "Implementierungsdetail, nicht anfassen" — bei uns vom
    // Compiler erzwungen, nicht nur Konvention.
    mod detail {
 
        // --- Datei: user_table.rs -------------------------------------------
        mod user_table {
            use crate::columns::{Dense, Idx};
 
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub struct UserId(usize);
            impl Idx for UserId {
                fn idx(self) -> usize {
                    self.0
                }
            }
 
            /// Das einzige Modul, das die privaten Felder von `UserTable` je
            /// sieht. `UserTableReader`/`UserReader` leben HIER drin — als
            /// Nachkommen bekommen sie direkten Feldzugriff, ganz ohne Brücke.
            mod detail {
                use super::{Dense, UserId};
 
                struct User {
                    name: String,
                    email: String,
                    verified: bool,
                    deleted: bool,
                }
 
                pub struct UserTable {
                    users: Dense<UserId, User>,
                }
 
                impl UserTable {
                    // ── Die einzigen drei `pub`-Methoden ─────────────────
                    pub fn new() -> Self {
                        Self {
                            users: Dense::new(),
                        }
                    }
                    pub fn read(&self) -> UserTableReader<'_> {
                        UserTableReader { table: self }
                    }
                    pub(in crate::app::detail) fn write(&mut self) -> super::UserTableWriter<'_> {
                        super::UserTableWriter { table: self }
                    }
 
                    // ── `pub(super)`: reicht bis `user_table` — genau dort
                    // leben `UserTableWriter`/`UserWriter`, die einzigen
                    // Nutzer dieser Methoden.
                    /// `UserId` wird HIER konstruiert — `Dense::push` liefert
                    /// nur die rohe Position, nie die ID selbst.
                    pub(super) fn create(&mut self, name: &str, email: &str) -> UserId {
                        let idx = self.users.push(User {
                            name: name.to_string(),
                            email: email.to_string(),
                            verified: false,
                            deleted: false,
                        });
                        UserId(idx)
                    }
                    pub(super) fn set_email(&mut self, id: UserId, new_email: &str) {
                        self.users.get_mut(id).email = new_email.to_string();
                    }
                    pub(super) fn verify(&mut self, id: UserId) {
                        self.users.get_mut(id).verified = true;
                    }
 
                    /// Auf dieser Ebene ganz normal — `UserTable` kennt keine
                    /// Posts, kann also nicht wissen, dass eine Löschung eine
                    /// Kaskade bräuchte. Der Schutz dafür sitzt eine Ebene
                    /// höher, in `StoreUserWriter`.
                    pub(super) fn remove(&mut self, id: UserId) {
                        self.users.get_mut(id).deleted = true;
                    }
                }
 
                pub struct UserTableReader<'a> {
                    table: &'a UserTable,
                }
                impl<'a> UserTableReader<'a> {
                    pub fn row(&self, id: UserId) -> UserReader<'a> {
                        UserReader {
                            table: self.table,
                            id,
                        }
                    }
                    // Direkter Feldzugriff — keine Brücke über `UserTable`
                    // nötig, weil Lesen sowieso nie riskant ist.
                    pub fn exists(&self, id: UserId) -> bool {
                        !self.table.users.get(id).deleted
                    }
                    pub fn find_by_email(&self, email: &str) -> Option<UserId> {
                        self.table
                            .users
                            .iter()
                            .find(|(_, u)| u.email == email && !u.deleted)
                            .map(|(idx, _)| UserId(idx))
                    }
                    /// Hartcodiert, bewusst KEIN generischer Filter — `UserTable`
                    /// kennt "verifiziert" als eigenständiges, benanntes Konzept.
                    /// Ein neuer echter Bedarf (z. B. "unverifiziert seit X Tagen")
                    /// bekommt eine EIGENE, neue, benannte Methode hier, nicht
                    /// eine beliebige Closure vom Aufrufer.
                    pub fn by_verified(&self, verified: bool) -> Vec<UserId> {
                        self.table
                            .users
                            .iter()
                            .filter(|(_, u)| u.verified == verified && !u.deleted)
                            .map(|(idx, _)| UserId(idx))
                            .collect()
                    }
                }
 
                pub struct UserReader<'a> {
                    table: &'a UserTable,
                    id: UserId,
                }
                impl UserReader<'_> {
                    pub fn name(&self) -> &str {
                        &self.table.users.get(self.id).name
                    }
                    pub fn email(&self) -> &str {
                        &self.table.users.get(self.id).email
                    }
                    pub fn is_verified(&self) -> bool {
                        self.table.users.get(self.id).verified
                    }
                }
            }
            pub use detail::{UserReader, UserTable};
 
            /// `pub(super)` — verlässt `user_table` nie, nur `Store`s eigene
            /// Methoden (eine Ebene höher, in `app`) nutzen das intern.
            pub(super) struct UserTableWriter<'a> {
                table: &'a mut UserTable,
            }
            impl<'a> UserTableWriter<'a> {
                pub fn row(self, id: UserId) -> UserWriter<'a> {
                    UserWriter {
                        table: self.table,
                        id,
                    }
                }
                pub fn create(&mut self, name: &str, email: &str) -> UserId {
                    self.table.create(name, email)
                }
                // Bewusst KEIN `remove`/`delete` hier.
            }
 
            /// `pub(super)` — der volle, löschfähige `UserWriter` verlässt
            /// `user_table` nie. `App` bekommt immer nur `StoreUserWriter`.
            pub(super) struct UserWriter<'a> {
                table: &'a mut UserTable,
                id: UserId,
            }
            impl UserWriter<'_> {
                pub fn set_email(&mut self, new_email: &str) {
                    self.table.set_email(self.id, new_email);
                }
                pub fn verify(&mut self) {
                    self.table.verify(self.id);
                }
                pub fn delete(self) {
                    self.table.remove(self.id);
                }
            }
        }
 
        // --- Datei: post_table.rs -------------------------------------------
        mod post_table {
            use super::user_table::UserId;
            use crate::columns::{Dense, Idx};
 
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub struct PostId(usize);
            impl Idx for PostId {
                fn idx(self) -> usize {
                    self.0
                }
            }
 
            mod detail {
                use super::{Dense, PostId, UserId};
 
                struct Post {
                    title: String,
                    body: String,
                    author: UserId,
                    deleted: bool,
                }
 
                pub struct PostTable {
                    posts: Dense<PostId, Post>,
                }
 
                impl PostTable {
                    pub fn new() -> Self {
                        Self {
                            posts: Dense::new(),
                        }
                    }
                    pub fn read(&self) -> PostTableReader<'_> {
                        PostTableReader { posts: self }
                    }
                    pub(in crate::app::detail) fn write(&mut self) -> super::PostTableWriter<'_> {
                        super::PostTableWriter { table: self }
                    }
 
                    fn by_author(&self, author: UserId) -> Vec<PostId> {
                        self.posts
                            .iter()
                            .filter(|(_, p)| p.author == author && !p.deleted)
                            .map(|(idx, _)| PostId(idx))
                            .collect()
                    }
 
                    pub(super) fn create(
                        &mut self,
                        title: &str,
                        body: &str,
                        author: UserId,
                    ) -> PostId {
                        let idx = self.posts.push(Post {
                            title: title.to_string(),
                            body: body.to_string(),
                            author,
                            deleted: false,
                        });
                        PostId(idx)
                    }
                    pub(super) fn set_title(&mut self, id: PostId, new_title: &str) {
                        self.posts.get_mut(id).title = new_title.to_string();
                    }
                    pub(super) fn reassign_author(&mut self, id: PostId, new_author: UserId) {
                        self.posts.get_mut(id).author = new_author;
                    }
                    pub(super) fn delete(&mut self, id: PostId) {
                        self.posts.get_mut(id).deleted = true;
                    }
                    pub(super) fn remove_by_author(&mut self, author: UserId) {
                        for id in self.by_author(author) {
                            self.delete(id);
                        }
                    }
                }
 
                pub struct PostTableReader<'a> {
                    posts: &'a PostTable,
                }
                impl<'a> PostTableReader<'a> {
                    pub fn post(&self, id: PostId) -> PostReader<'a> {
                        PostReader {
                            table: self.posts,
                            id,
                        }
                    }
                    pub fn by_author(&self, author: UserId) -> Vec<PostId> {
                        self.posts.by_author(author)
                    }
                }
 
                pub struct PostReader<'a> {
                    table: &'a PostTable,
                    id: PostId,
                }
                impl PostReader<'_> {
                    pub fn title(&self) -> &str {
                        &self.table.posts.get(self.id).title
                    }
                    pub fn author_id(&self) -> UserId {
                        self.table.posts.get(self.id).author
                    }
                }
            }
            pub use detail::{PostReader, PostTable};
 
            pub(super) struct PostTableWriter<'a> {
                table: &'a mut PostTable,
            }
            impl<'a> PostTableWriter<'a> {
                pub fn post(self, id: PostId) -> PostWriter<'a> {
                    PostWriter {
                        posts: self.table,
                        id,
                    }
                }
                pub fn create(&mut self, title: &str, body: &str, author: UserId) -> PostId {
                    self.table.create(title, body, author)
                }
                pub fn delete(&mut self, id: PostId) {
                    self.table.delete(id);
                }
                pub fn remove_by_author(&mut self, author: UserId) {
                    self.table.remove_by_author(author);
                }
            }
 
            pub(super) struct PostWriter<'a> {
                posts: &'a mut PostTable,
                id: PostId,
            }
            impl PostWriter<'_> {
                pub fn set_title(&mut self, new_title: &str) {
                    self.posts.set_title(self.id, new_title);
                }
                pub fn reassign_author(&mut self, new_author: UserId) {
                    self.posts.reassign_author(self.id, new_author);
                }
            }
        }
 
        // --- Datei: store.rs — Store selbst; user_table/post_table sind jetzt
        // KINDER von hier, kein "super::" mehr nötig, um sie zu erreichen ------
        use post_table::PostTable;
        pub use post_table::{PostId, PostReader};
        use user_table::UserTable;
        pub use user_table::{UserId, UserReader};
 
        pub struct Store {
            users: UserTable,
            posts: PostTable,
        }
 
        impl Store {
            pub fn new() -> Self {
                Self {
                    users: UserTable::new(),
                    posts: PostTable::new(),
                }
            }
            pub fn read(&self) -> StoreReader<'_> {
                StoreReader { store: self }
            }
            pub(super) fn write(&mut self) -> super::StoreWriter<'_> {
                super::StoreWriter { store: self }
            }
 
            pub(super) fn users(&self) -> &UserTable {
                &self.users
            }
            /// Gibt `app` nur noch `(&mut Store, id)`, NIE mehr den rohen
            /// `UserWriter` selbst — der ist (dank `user_table` als
            /// Kind von `detail`) für `app` gar nicht mehr benennbar.
            pub(super) fn user_writer(&mut self, id: UserId) -> super::StoreUserWriter<'_> {
                super::StoreUserWriter { store: self, id }
            }
            pub(super) fn post_writer(&mut self, id: PostId) -> super::StorePostWriter<'_> {
                super::StorePostWriter { store: self, id }
            }
            // ── Dünne Forwarding-Methoden: der einzige Ort, an dem
            // `UserWriter`/`PostWriter` außerhalb ihrer eigenen Definition
            // noch direkt angefasst werden — und das darf hier passieren,
            // weil `detail` der direkte ELTERNTEIL von `user_table`
            // bzw. `post_table` ist (nicht nur ein Geschwister).
            pub(super) fn set_user_email(&mut self, id: UserId, new_email: &str) {
                self.users.write().row(id).set_email(new_email);
            }
            pub(super) fn verify_user_writer(&mut self, id: UserId) {
                self.users.write().row(id).verify();
            }
            pub(super) fn set_post_title(&mut self, id: PostId, new_title: &str) {
                self.posts.write().post(id).set_title(new_title);
            }
            pub(super) fn reassign_post_author(&mut self, id: PostId, new_author: UserId) {
                self.posts.write().post(id).reassign_author(new_author);
            }
            pub(super) fn create_user(&mut self, name: &str, email: &str) -> UserId {
                self.users.write().create(name, email)
            }
            pub(super) fn create_post(
                &mut self,
                title: &str,
                body: &str,
                author: UserId,
            ) -> Result<PostId, String> {
                if !self.users.read().exists(author) {
                    return Err(format!("author {:?} does not exist", author));
                }
                Ok(self.posts.write().create(title, body, author))
            }
            pub(super) fn delete_post(&mut self, id: PostId) {
                self.posts.write().delete(id);
            }
            pub(super) fn delete_orphans_of(&mut self, id: UserId) -> usize {
                let count = self.posts.read().by_author(id).len();
                self.posts.write().remove_by_author(id);
                self.users.write().row(id).delete();
                count
            }
            pub(super) fn delete_and_reassign_orphans(
                &mut self,
                id: UserId,
                new_author: UserId,
            ) -> Result<usize, String> {
                if !self.users.read().exists(new_author) {
                    return Err(format!(
                        "reassignment target {:?} does not exist",
                        new_author
                    ));
                }
                let affected = self.posts.read().by_author(id);
                let count = affected.len();
                for post_id in affected {
                    self.posts.write().post(post_id).reassign_author(new_author);
                }
                self.users.write().row(id).delete();
                Ok(count)
            }
        }
 
        pub struct StoreReader<'a> {
            store: &'a Store,
        }
        impl<'a> StoreReader<'a> {
            pub fn user(&self, id: UserId) -> UserReader<'a> {
                self.store.users.read().row(id)
            }
            pub fn post(&self, id: PostId) -> PostReader<'a> {
                self.store.posts.read().post(id)
            }
            pub fn find_user_by_email(&self, email: &str) -> Option<UserId> {
                self.store.users.read().find_by_email(email)
            }
            pub fn user_exists(&self, id: UserId) -> bool {
                self.store.users.read().exists(id)
            }
            pub fn posts_by(&self, author: UserId) -> Vec<PostId> {
                self.store.posts.read().by_author(author)
            }
            /// Hartcodiert, bewusst KEIN generischer Filter — das Wissen,
            /// was "verifizierte User mit einer bestimmten Domain"
            /// bedeutet, gehört hierher, nicht zu `App`. Taucht morgen ein
            /// echter neuer Bedarf auf, bekommt der eine EIGENE, neue,
            /// benannte Methode — kein beliebiges Aufrufer-Prädikat.
            pub fn verified_users_with_domain(&self, domain: &str) -> Vec<UserId> {
                self.store
                    .users
                    .read()
                    .by_verified(true)
                    .into_iter()
                    .filter(|&id| self.store.users.read().row(id).email().ends_with(domain))
                    .collect()
            }
        }
    }
    pub use detail::{PostId, PostReader, Store, StoreReader, UserId, UserReader};
 
    // ── Ab hier: die Store-Writer-Familie. Liegt im GENAU GLEICHEN Modul
    // wie `App` selbst — braucht deshalb gar kein Sichtbarkeits-Schlüsselwort.
 
    struct StoreWriter<'a> {
        store: &'a mut Store,
    }
    impl<'a> StoreWriter<'a> {
        fn user(&mut self, id: UserId) -> StoreUserWriter<'_> {
            self.store.user_writer(id)
        }
        fn post(&mut self, id: PostId) -> StorePostWriter<'_> {
            self.store.post_writer(id)
        }
        /// `self` per WERT — genau wie bei `row(self, ...)`: `store: &'a mut
        /// Store` ist nicht `Copy`, also muss es aus `self` HERAUSBEWEGT
        /// werden, um die volle Lifetime `'a` weiterzugeben. Nötig, weil
        /// `App::retire_user` das Ergebnis über die Funktion hinaus
        /// zurückgibt (als `RetireUserAction`) — mit `&mut self` würde die
        /// Lifetime auf die kurze Lebensdauer der ungebundenen
        /// `self.store.write()`-Zwischeninstanz verkürzt.
        fn users(self) -> StoreUsersWriter<'a> {
            StoreUsersWriter { store: self.store }
        }
        fn posts(&mut self) -> StorePostsWriter<'_> {
            StorePostsWriter { store: self.store }
        }
    }
 
    /// Hält `Store` + `id`, NICHT mehr `user_table::UserWriter` — der Typ
    /// ist seit dem Umbau gar nicht mehr benennbar (er ist
    /// `pub(super)` in `user_table`, reicht also nur bis `detail`). Jede
    /// Mutation läuft über Store's eigene, bewusst begrenzte Methoden.
    struct StoreUserWriter<'a> {
        store: &'a mut Store,
        id: UserId,
    }
    impl StoreUserWriter<'_> {
        fn email(&mut self, new_email: &str) {
            self.store.set_user_email(self.id, new_email);
        }
        fn verify(&mut self) {
            self.store.verify_user_writer(self.id);
        }
        // Bewusst KEIN `delete()` hier — und diesmal nicht nur aus
        // Disziplin: `Store` bietet dafür schlicht keine Methode an, also
        // gibt es hier auch keinen Weg, versehentlich doch dranzukommen.
    }
 
    struct StorePostWriter<'a> {
        store: &'a mut Store,
        id: PostId,
    }
    impl StorePostWriter<'_> {
        fn title(&mut self, new_title: &str) {
            self.store.set_post_title(self.id, new_title);
        }
        fn reassign_author(&mut self, new_author: UserId) {
            self.store.reassign_post_author(self.id, new_author);
        }
    }
 
    struct StoreUsersWriter<'a> {
        store: &'a mut Store,
    }
    impl<'a> StoreUsersWriter<'a> {
        fn create(&mut self, name: &str, email: &str) -> UserId {
            self.store.create_user(name, email)
        }
        fn delete(self, id: UserId) -> DeleteUserWriter<'a> {
            DeleteUserWriter {
                store: self.store,
                target: id,
            }
        }
        /// Hartcodiert, wie `UserTableReader::by_verified` — Store kennt
        /// "verifiziert" als eigenständiges Konzept, statt generisch eine
        /// beliebige Bedingung vom Aufrufer entgegenzunehmen.
        fn by_verified(self, verified: bool) -> VerifiedUsersWriter<'a> {
            VerifiedUsersWriter {
                store: self.store,
                verified,
            }
        }
    }
 
    struct VerifiedUsersWriter<'a> {
        store: &'a mut Store,
        verified: bool,
    }
    impl VerifiedUsersWriter<'_> {
        fn delete_each(self, mut f: impl FnMut(DeleteUserWriter<'_>)) {
            let matching = self.store.users().read().by_verified(self.verified);
            for id in matching {
                f(DeleteUserWriter {
                    store: self.store,
                    target: id,
                });
            }
        }
        /// `cascade()` gibt die Anzahl gelöschter POSTS zurück (praktisch
        /// für `retire_user`, wo genau das interessiert). Hier zählen wir
        /// stattdessen selbst mit — `purge_unverified_users` will wissen,
        /// wie viele USER entfernt wurden, nicht wie viele Posts dabei
        /// mitgelöscht wurden. Das war der eigentliche Bug: vorher wurde
        /// `cascade()`s Post-Zahl aufaddiert statt der User-Anzahl.
        fn delete_cascade(self) -> usize {
            let mut total = 0;
            self.delete_each(|w| {
                w.cascade();
                total += 1;
            });
            total
        }
    }
 
    struct StorePostsWriter<'a> {
        store: &'a mut Store,
    }
    impl StorePostsWriter<'_> {
        fn new(&mut self, title: &str, body: &str, author: UserId) -> Result<PostId, String> {
            self.store.create_post(title, body, author)
        }
        fn delete(&mut self, id: PostId) {
            self.store.delete_post(id);
        }
    }
 
    struct DeleteUserWriter<'a> {
        store: &'a mut Store,
        target: UserId,
    }
    impl DeleteUserWriter<'_> {
        fn cascade(self) -> usize {
            self.store.delete_orphans_of(self.target)
        }
        fn reassign_orphans_to(self, new_author: UserId) -> Result<usize, String> {
            self.store
                .delete_and_reassign_orphans(self.target, new_author)
        }
    }
 
    /// Öffentliche Hülle um den privaten `DeleteUserWriter` — hält ihn
    /// versteckt, damit `App` die einzige Tür nach außen bleibt. Das
    /// API-Design von `App` ist damit komplett frei wählbar, unabhängig
    /// davon, wie `Store`s interne Typen heißen oder geschnitten sind.
    pub struct RetireUserAction<'a> {
        inner: DeleteUserWriter<'a>,
    }
    impl RetireUserAction<'_> {
        pub fn delete_orphaned(self) -> usize {
            self.inner.cascade()
        }
        pub fn reassign_orphaned_to(self, new_owner: UserId) -> Result<usize, String> {
            self.inner.reassign_orphans_to(new_owner)
        }
    }
 
    // =========================================================================
    // APP
    // =========================================================================
 
    pub struct App {
        store: Store,
    }
 
    pub struct UserProfile {
        pub name: String,
        pub email: String,
        pub post_count: usize,
    }
 
    impl App {
        pub fn new() -> Self {
            Self {
                store: Store::new(),
            }
        }
 
        pub fn register_user(&mut self, name: &str, email: &str) -> UserId {
            self.store.write().users().create(name, email)
        }
 
        pub fn publish_post(
            &mut self,
            author: UserId,
            title: &str,
            body: &str,
        ) -> Result<PostId, String> {
            if !self.store.read().user(author).is_verified() {
                return Err("only verified users can publish posts".to_string());
            }
            self.store.write().posts().new(title, body, author)
        }
 
        pub fn verify_user(&mut self, id: UserId) {
            self.store.write().user(id).verify();
        }
 
        pub fn update_email(&mut self, id: UserId, new_email: &str) {
            self.store.write().user(id).email(new_email);
        }
 
        pub fn edit_post_title(&mut self, id: PostId, new_title: &str) {
            self.store.write().post(id).title(new_title);
        }
 
        /// Gibt eine `RetireUserAction` zurück, statt sofort zu handeln —
        /// der Aufrufer entscheidet per `.delete_orphaned()` oder
        /// `.reassign_orphaned_to(...)`, was mit den Posts passiert.
        pub fn retire_user(&mut self, id: UserId) -> RetireUserAction<'_> {
            RetireUserAction {
                inner: self.store.write().users().delete(id),
            }
        }
 
        pub fn purge_unverified_users(&mut self) -> usize {
            self.store
                .write()
                .users()
                .by_verified(false)
                .delete_cascade()
        }
 
        pub fn reassign_all_unverified_to(&mut self, new_owner: UserId) -> Result<usize, String> {
            let mut total = 0;
            let mut first_error = None;
            self.store
                .write()
                .users()
                .by_verified(false)
                .delete_each(|w| match w.reassign_orphans_to(new_owner) {
                    Ok(count) => total += count,
                    Err(e) => {
                        first_error.get_or_insert(e);
                    }
                });
            match first_error {
                Some(e) => Err(e),
                None => Ok(total),
            }
        }
 
        pub fn remove_post(&mut self, id: PostId) {
            self.store.write().posts().delete(id);
        }
 
        pub fn user_profile(&self, id: UserId) -> UserProfile {
            UserProfile {
                name: self.store.read().user(id).name().to_string(),
                email: self.store.read().user(id).email().to_string(),
                post_count: self.store.read().posts_by(id).len(),
            }
        }
 
        pub fn post_title(&self, id: PostId) -> String {
            self.store.read().post(id).title().to_string()
        }
 
        pub fn find_user_by_email(&self, email: &str) -> Option<UserId> {
            self.store.read().find_user_by_email(email)
        }
 
        /// `App` sagt nur noch das WAS ("verifizierte User mit dieser
        /// Domain"), nicht das WIE — die Kombination der beiden
        /// Bedingungen ist Wissen, das bei `Store` liegt.
        pub fn count_verified_users_with_domain(&self, domain: &str) -> usize {
            self.store.read().verified_users_with_domain(domain).len()
        }
    }
}
 
pub use app::App;
pub use app::{PostId, PostReader, Store, StoreReader, UserId, UserReader};
 
fn main() {
    let mut app = App::new();
 
    let alice = app.register_user("Alice", "alice@example.com");
    app.verify_user(alice);
    let post1 = app
        .publish_post(alice, "Hello Rust", "My first post!")
        .unwrap();
    app.publish_post(alice, "Modules 101", "...").unwrap();
 
    println!("--- Columns sind wieder da ---");
    let profile = app.user_profile(alice);
    println!(
        "{} ({}) — {} Posts",
        profile.name, profile.email, profile.post_count
    );
 
    app.edit_post_title(post1, "Hello Rust (Updated)");
    println!("neuer Titel: {}", app.post_title(post1));
 
    let bob = app.register_user("Bob", "bob@example.com");
    app.verify_user(bob);
    app.publish_post(bob, "Bob's post", "...").unwrap();
    let reassigned = app.retire_user(bob).reassign_orphaned_to(alice).unwrap();
    println!("{} Post(s) an Alice umgehängt", reassigned);
 
    let eve = app.register_user("Eve", "eve@example.com");
    app.verify_user(eve);
    app.publish_post(eve, "Eve's post", "...").unwrap();
    let deleted = app.retire_user(eve).delete_orphaned();
    println!("{} Post(s) mit Eve gelöscht", deleted);
 
    let charlie = app.register_user("Charlie", "charlie@example.com");
    let dana = app.register_user("Dana", "dana@example.com");
    let peter = app.register_user("peter", "peter@example.com");
    let hubert = app.register_user("hubert", "hubert@example.com");
    app.verify_user(dana);
    let purged = app.purge_unverified_users();
    println!("{} unverifizierte(r) User entfernt", purged);
    println!("Dana existiert weiterhin: {}", app.user_profile(dana).name);
    let _ = charlie;
    let _ = peter;
    let _ = hubert;
 
    println!("\n--- verified_users_with_domain() — Store kennt die Kombination, nicht App ---");
    let frank = app.register_user("Frank", "frank@example.com");
    app.verify_user(frank);
    let count = app.count_verified_users_with_domain("@example.com");
    println!("Verifizierte User mit @example.com: {}", count);
 
    println!("\n--- Was mit einer UserId von außen NICHT möglich ist ---");
    // let fake = app::user_table::UserId(999);       // <- Fehler: Modul UND Feld privat
    // let fake2: UserId = columns::Idx::idx(...);      // <- gibt's nicht, from_idx existiert nicht mehr
    println!("(UserId nur innerhalb von `user_table` konstruierbar — `Dense` kann keine erfinden)");
}
