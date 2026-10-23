; Regras do Firewall do Windows para quem vira host receber conexões
; (API/sinalização TCP 7880, mídia TCP 7881 e UDP 7882, descoberta na rede local UDP 7883).

!macro customInstall
  nsExec::Exec 'netsh advfirewall firewall delete rule name="CompartilhadorDeTela"'
  nsExec::Exec 'netsh advfirewall firewall add rule name="CompartilhadorDeTela" dir=in action=allow program="$INSTDIR\CompartilhadorDeTela.exe" enable=yes profile=any'
  nsExec::Exec 'netsh advfirewall firewall add rule name="CompartilhadorDeTela" dir=in action=allow program="$INSTDIR\resources\bin\livekit-server.exe" enable=yes profile=any'
!macroend

!macro customUnInstall
  nsExec::Exec 'netsh advfirewall firewall delete rule name="CompartilhadorDeTela"'
!macroend
