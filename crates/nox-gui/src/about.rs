//! About Nox (main menu): version, license, links, and the third-party
//! work Nox bundles (docs/CREDITS.md).

use adw::prelude::*;

use crate::i18n::gettext;

/// Shows the About dialog over `parent`.
pub fn show(parent: Option<&gtk::Window>) {
    let about = adw::AboutDialog::builder()
        .application_name("Nox")
        .application_icon(crate::APP_ID)
        .developer_name("Stephan Vedder")
        .version(env!("CARGO_PKG_VERSION"))
        .website("https://github.com/feliwir/nox_rs")
        .issue_url("https://github.com/feliwir/nox_rs/issues")
        .license_type(gtk::License::Gpl30)
        .copyright("© 2026 Stephan Vedder")
        .build();
    // Translators: your names, one per line, as "Name <email>".
    let translators = gettext("translator-credits");
    if translators != "translator-credits" {
        about.set_translator_credits(&translators);
    }
    about.add_legal_section(
        "gl-transitions",
        Some("© 2017 gl-transitions contributors"),
        gtk::License::MitX11,
        None,
    );
    about.add_legal_section(
        "Noto Sans",
        Some("© The Noto Project Authors"),
        gtk::License::Custom,
        Some(&gettext("SIL Open Font License, version 1.1")),
    );
    about.present(parent);
}
