plugins_conf = configuration_data()
plugins_conf.set('APP_ID', application_id)

configure_file(
  input: 'packet_nautilus.py.in',
  output: 'packet_nautilus.py',
  configuration: plugins_conf,
  install_dir: pkgdatadir / 'plugins',
)