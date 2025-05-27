set ignore-comments

default:
    @just --list

# Generate packet.pot
pot:
    # xgettext seems to follow `translatable="no"` in the metainfo file too, so that's nice
    #
    # For `.blp`
    # --keyword=_ \
    # --keyword=C_:1c,2 \
    xgettext \
        --from-code=UTF-8 \
        --add-comments \
        --keyword=_ \
        --keyword=C_:1c,2 \
        --package-name packet \
        --default-domain packet \
        --files-from {{ source_directory() / "po" / "POTFILES.in" }} \
        --output {{ source_directory() / "po" / "packet.pot" }} \

    # https://github.com/mesonbuild/meson/issues/12368
    # cat "{{ source_directory() / "po" / "LINGUAS" }}" | while read line; do touch "{{ source_directory() / "po" }}/${line}.po"; done
    # https://mesonbuild.com/Localisation.html
    # meson compile packet-pot
    # meson compile packet-update-po

