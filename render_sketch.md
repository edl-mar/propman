# Render sketch

Key set used below (example files + some hypothetical deeper keys added for illustration):

    app.confirm.delete
    app.confirm.discard
    app.loading
    app.status.error
    app.status.saved
    app.title
    app.version
    com.myapp.error.notfound
    com.myapp.error.timeout
    com.myapp.error.unauthorized
    com.myapp.error.unexpected
    com.myapp.ui.button.abort
    com.myapp.ui.button.confirm
    com.myapp.ui.button.delete
    com.myapp.ui.button.save
    com.myapp.ui.label.created
    com.myapp.ui.label.email
    com.myapp.ui.label.name
    com.myapp.ui.placeholder.name
    com.myapp.ui.placeholder.search
    com.myapp.admin.user.permission.read      ← hypothetical, 6 levels deep
    com.myapp.admin.user.permission.write
    com.myapp.admin.user.permission.delete
    com.myapp.admin.settings.theme
    com.myapp.admin.settings.language

---

## Option A — branch-point headers only (emit header when ≥2 children)

Lone keys under a prefix are shown in full without a header.
Single-child chains collapse: "com.myapp" never gets its own header line.

    app.confirm:
      .delete:  [default] Are you sure you want to delete this item?  [de] <missing>
      .discard: [default] You have unsaved changes. Discard them?  [de] <missing>
    app.loading: [default] Loading…  [de] <missing>
    app.status:
      .error: [default] Could not save: {0}  [de] <missing>
      .saved: [default] Changes saved  [de] <missing>
    app.title:   [default] Property Manager  [de] <missing>
    app.version: [default] Version {0}  [de] <missing>
    com.myapp.admin.settings:
      .language: [default] …  [de] <missing>
      .theme:    [default] …  [de] <missing>
    com.myapp.admin.user.permission:
      .delete: [default] …  [de] <missing>
      .read:   [default] …  [de] <missing>
      .write:  [default] …  [de] <missing>
    com.myapp.error:
      .notfound:     [default] Not found  [de] Nicht gefunden  [si] Ni najdeno
      .timeout:      [default] Timeout  [de] Zeitüberschreitung  [si] Prekoračitev časa
      .unauthorized: [default] You are not authorized…  [de] Sie sind nicht…  [si] <missing>
      .unexpected:   [default] An unexpected error…  [de] Ein unerwarteter…  [si] <missing>
    com.myapp.ui.button:
      .abort:   [default] Abort  [de] Abbrechen  [si] <missing>
      .confirm: [default] Confirm  [de] <missing>  [si] <missing>
      .delete:  [default] Delete  [de] Löschen  [si] <missing>
      .save:    [default] Save  [de] Speichern  [si] Shrani
    com.myapp.ui.label:
      .created: [default] Created  [de] Erstellt  [si] <missing>
      .email:   [default] Email address  [de] E-Mail-Adresse  [si] E-poštni naslov
      .name:    [default] Name  [de] Name  [si] Ime
    com.myapp.ui.placeholder:
      .name:   [default] Enter your name  [de] <missing>  [si] <missing>
      .search: [default] Search…  [de] Suchen…  [si] <missing>

Note: app.loading / app.title / app.version are not grouped — they share no 2-child prefix
with each other. Deeply nested lone chains like "com.myapp.admin.user.permission" are
flattened into one header rather than one header per level.

---

## Option B — full tree (emit a header for every internal node that has children)

Every level gets its own header line. Indentation grows by 2 spaces per level.

    app:
      .confirm:
        .delete:  [default] Are you sure you want to delete this item?  [de] <missing>
        .discard: [default] You have unsaved changes. Discard them?  [de] <missing>
      .loading: [default] Loading…  [de] <missing>
      .status:
        .error: [default] Could not save: {0}  [de] <missing>
        .saved: [default] Changes saved  [de] <missing>
      .title:   [default] Property Manager  [de] <missing>
      .version: [default] Version {0}  [de] <missing>
    com:
      .myapp:
        .admin:
          .settings:
            .language: [default] …  [de] <missing>
            .theme:    [default] …  [de] <missing>
          .user:
            .permission:
              .delete: [default] …  [de] <missing>
              .read:   [default] …  [de] <missing>
              .write:  [default] …  [de] <missing>
        .error:
          .notfound:     [default] Not found  [de] Nicht gefunden  [si] Ni najdeno
          .timeout:      [default] Timeout  [de] Zeitüberschreitung  [si] Prekoračitev časa
          .unauthorized: [default] You are not authorized…  [de] Sie sind nicht…  [si] <missing>
          .unexpected:   [default] An unexpected error…  [de] Ein unerwarteter…  [si] <missing>
        .ui:
          .button:
            .abort:   [default] Abort  [de] Abbrechen  [si] <missing>
            .confirm: [default] Confirm  [de] <missing>  [si] <missing>
            .delete:  [default] Delete  [de] Löschen  [si] <missing>
            .save:    [default] Save  [de] Speichern  [si] Shrani
          .label:
            .created: [default] Created  [de] Erstellt  [si] <missing>
            .email:   [default] Email address  [de] E-Mail-Adresse  [si] E-poštni naslov
            .name:    [default] Name  [de] Name  [si] Ime
          .placeholder:
            .name:   [default] Enter your name  [de] <missing>  [si] <missing>
            .search: [default] Search…  [de] Suchen…  [si] <missing>

Note: com.myapp.admin.user.permission keys end up 6 levels deep (12 spaces of indent).
The single-child chain com → myapp → admin produces three header-only lines before
any key is visible, which could feel noisy for long chains.
