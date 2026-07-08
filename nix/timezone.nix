# Timezone provisioning, following the wifi-ap.env pattern: a oneshot
# service applies /boot/firmware/timezone.env (TIMEZONE=<IANA zone>) at
# boot, falling back to Europe/Madrid when the file (or the variable) is
# absent. Drop a timezone.env on a card's FAT boot partition to override —
# no image rebuild needed.
#
# Note this only fixes the *zone* (UTC vs CEST display offset). The Pis are
# RTC-less and offline, so a wall clock that is outright wrong still needs
# the clock set (coin cell on J5, or pushing time over ethernet).
{ pkgs, ... }:
{
  # Released to imperative control so timedatectl can set it at boot.
  time.timeZone = null;

  systemd.services.timezone-provision = {
    description = "Set timezone from the card's timezone.env (default Europe/Madrid)";
    wantedBy = [ "multi-user.target" ];
    after = [ "dbus.service" ];
    path = [ pkgs.systemd ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      set -eu
      tz="Europe/Madrid"
      if [ -f /boot/firmware/timezone.env ]; then
        . /boot/firmware/timezone.env
        tz="''${TIMEZONE:-$tz}"
      fi
      timedatectl set-timezone "$tz"
    '';
  };
}
