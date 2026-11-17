<#macro emailLayout>
<!DOCTYPE html>
<html lang="${locale.language}">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="light">
  <style>a { color: #0a0a0a; text-decoration: underline; }</style>
</head>
<body style="margin:0;padding:0;background-color:#fafafa;">
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background-color:#fafafa;padding:32px 16px;">
    <tr>
      <td align="center">
        <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:560px;">
          <tr>
            <td style="padding:0 0 16px 4px;font-family:Roboto,'Helvetica Neue',Arial,sans-serif;font-size:22px;font-weight:700;letter-spacing:-0.01em;color:#0a0a0a;">
              <span style="display:inline-block;width:32px;height:32px;line-height:32px;border-radius:8px;background-color:#2563eb;color:#ffffff;text-align:center;font-size:18px;vertical-align:middle;">Z</span>
              <span style="vertical-align:middle;padding-left:8px;">Zeima</span>
            </td>
          </tr>
          <tr>
            <td style="background-color:#ffffff;border:1px solid #e5e5e5;border-radius:10px;padding:28px 28px 24px 28px;font-family:Roboto,'Helvetica Neue',Arial,sans-serif;font-size:15px;line-height:1.55;color:#0a0a0a;">
              <#nested>
            </td>
          </tr>
          <tr>
            <td style="padding:16px 4px 0 4px;font-family:Roboto,'Helvetica Neue',Arial,sans-serif;font-size:12px;line-height:1.5;color:#737373;">
              Zeima
            </td>
          </tr>
        </table>
      </td>
    </tr>
  </table>
</body>
</html>
</#macro>
