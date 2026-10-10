# i18n.gettext reads LINGUAS at configure time only: tracking it as an input
# makes a change to it (a new language) reconfigure the build.
configure_file(input: 'LINGUAS', output: 'LINGUAS.stamp', copy: true)
i18n.gettext('vinheta', preset: 'glib')
