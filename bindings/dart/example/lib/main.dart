// The smallest Flutter app on teigeodata: the platform folders are made by `flutter create .`
// when it is built (`cargo xtask flutter-check`), so only this and the test are kept.
import 'package:flutter/widgets.dart';
import 'package:teigeodata/teigeodata.dart';

void main() => runApp(const App());

/// Shows the library's version and a folded name.
class App extends StatelessWidget {
  const App({super.key});

  @override
  Widget build(BuildContext context) {
    final tg = TeiGeodata();
    return Directionality(
      textDirection: TextDirection.ltr,
      child: Center(child: Text('teigeodata ${tg.version()}: ${tg.fold('Lahachowk')}')),
    );
  }
}
