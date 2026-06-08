import { Routes } from '@angular/router';
import { LoadingComponent } from './loading/loading.component';
import { ShellComponent } from './shell/shell.component';

export const routes: Routes = [
  { path: '', component: LoadingComponent },
  { path: 'app', component: ShellComponent },
  { path: '**', redirectTo: '' },
];
